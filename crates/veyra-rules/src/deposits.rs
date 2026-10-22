//! Deposits in the sim: the ones near the observers, the purse, what the
//! household has found and the requests that find and sample (ticket 0007,
//! step 3; ticket 0009, step 3).
//!
//! Where the deposits are is the world's (`veyra_world::deposits`), a
//! pure function the game hands the sim as a [`Survey`]. A server has none
//! and so has no deposits: nothing here is replicated.
//! `refresh_near_deposits` keeps [`NearDeposits`], every deposit within
//! the register's range of an observer, the way the site list is kept: on
//! the task pool, and only when an observer has moved.
//!
//! A lot is a stack ([`lot`]): the type's lot item, the units as its count,
//! the deposit's figures as its lines and the deposit itself as its origin,
//! which is in the patch, so two lots merge only when they are of one
//! deposit and never mix silently.
//!
//! Nothing is published to the household: a deposit is found by a ping
//! (`hand`) with a carried survey tool, and sampled by hand. What has been
//! found is [`Surveyed::found`]; the register lists that and nothing else.
//! A request is a `SurveyRequest` in the `SurveyInbox`, consumed by
//! `consume` and answered with a message. Everything a screen shows is
//! asked here and not worked out again: [`hand::can_ping`] and
//! [`hand::can_sample`] for a button and its reason, [`lot_lines`] for a
//! card, [`register`] for the register with its distances and bearings. A
//! screen decides no rule.

/// Public for the adapter half in `veyra-sim`, which calls it.
pub mod hand;

pub use hand::*;

use std::collections::BTreeMap;

use std::sync::Arc;

use bevy_ecs::prelude::*;

use strata_registry::item::{ItemCatalogue, ItemKind, LotOrigin, Record, lot_item};

use strata_registry::{Attribute, Namespaced, Reading, ResourceSet, Span};

pub use veyra_world::deposits::{Deposit, DepositField, DepositId};

pub use crate::household::Purse;

use crate::items::{self, Hands, Pack, Stock};

/// The world's deposits, published by the game once its world exists. A
/// server and a harness have none.
#[derive(Resource, Debug, Clone)]
pub struct Survey(pub Arc<DepositField>);

/// The deposits within the register's range of the observers, and where
/// the list was last made round.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct NearDeposits {
    /// The deposits, ordered by id: the same order everywhere.
    pub deposits: Vec<Deposit>,
    /// The observers' places when it was made, `(x, z)`.
    pub around: Vec<[f64; 2]>,
}

impl NearDeposits {
    /// The deposit `id`, if it is in the list.
    #[must_use]
    pub fn get(&self, id: DepositId) -> Option<&Deposit> {
        self.deposits.iter().find(|deposit| deposit.id == id)
    }
}

/// What the household has found of one deposit.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Found {
    /// How well it is read; never lowered once known.
    pub reading: Reading,
    /// Whether the household has sampled it, which reads it exactly.
    pub sampled: bool,
    /// The strongest concentration a ping has met in it, per-mille.
    pub best_pm: u16,
    /// Where that was, `(x, z)`.
    pub best_at: [f64; 2],
}

/// The strongest point of the last ping, which the HUD points to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lead {
    /// The deposit holding it.
    pub deposit: DepositId,
    /// Where, `(x, z)`.
    pub at: [f64; 2],
    /// The concentration there, per-mille.
    pub pm: u16,
}

/// What the household knows of the survey: the deposits it has found, the
/// last ping's lead and the one it tracks. The game saves the found and the
/// tracked with the household.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Surveyed {
    /// Every deposit a ping has met, with what is known of it.
    pub found: BTreeMap<DepositId, Found>,
    /// The deposit the HUD points to, whole, so the pointer needs no
    /// lookup while it is far outside [`NearDeposits`].
    pub tracked: Option<Deposit>,
    /// The last ping's strongest point, or none when it met nothing.
    pub lead: Option<Lead>,
}

/// Present when the household's survey record is not kept this run (it is
/// of another world), so "found" and "first" cannot be told once and a
/// restart would pay the same finds again: no Survey XP is credited.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SurveyUnkept;

/// The lot `units` of `deposit` make: the type's lot item, the
/// deposit's figures on its lines and the deposit as its origin. `None`
/// for a deposit of a type `set` has not got.
#[must_use]
pub fn lot(deposit: &Deposit, set: &ResourceSet, units: u32) -> Option<Record> {
    let def = set.get(deposit.ty)?;
    Some(Record {
        item: lot_item(&def.id)?,
        count: units,
        batch: 0,
        by: None,
        lines: deposit
            .figures
            .iter()
            .map(|(attribute, value)| (attribute.key().to_owned(), *value))
            .collect(),
        condition: None,
        fitted: BTreeMap::new(),
        origin: Some(LotOrigin {
            deposit: deposit.id.0,
            root: deposit.root.clone(),
            place: deposit.place.clone(),
        }),
    })
}

/// The deposit `from` (`x`, `z`) stands at: the nearest of `near` within
/// the dials' `place_m` of its centre, the lower id on a tie. What the HUD
/// names the place by.
#[must_use]
pub fn standing_at<'a>(
    near: &'a NearDeposits,
    set: &ResourceSet,
    from: [f64; 2],
) -> Option<&'a Deposit> {
    let within = set.rules().place.place_m;
    near.deposits
        .iter()
        .map(|deposit| (deposit.distance_m(from[0], from[1]), deposit))
        .filter(|(distance, _)| *distance <= within)
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)))
        .map(|(_, deposit)| deposit)
}

/// The pack with the lot put in it, or `None` when any of it found no
/// room: a lot is taken whole or not at all.
pub fn with_lot(items: &ItemCatalogue, pack: &Pack, record: &Record) -> Option<Pack> {
    let lot = items::write(record, items).ok()?;
    let mut next = pack.clone();
    // The rule that puts anything away puts a lot away: onto a stack of
    // its own deposit first, then the pack's cells, then a bag's. No store: a
    // field sample is not a locker.
    let mut hands = Hands::default();
    let mut stock = Stock {
        saddle: None,
        pack: &mut next,
        hands: &mut hands,
        store: None,
    };
    items::place(items, &mut stock, lot)
        .is_none()
        .then_some(next)
}

/// One line of a lot's card: the attribute, the lot's figure and the
/// type's span it lies in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LotLine {
    /// The attribute.
    pub attribute: Attribute,
    /// The lot's figure.
    pub value: u16,
    /// The type's floor and cap for it.
    pub span: Span,
}

/// A lot's lines in its class's order, each with its type's span. Empty
/// for a record that is no lot.
#[must_use]
pub fn lot_lines(items: &ItemCatalogue, record: &Record) -> Vec<LotLine> {
    let Some(ItemKind::Lot { ranges, .. }) = items.get(&record.item).map(|def| &def.kind) else {
        return Vec::new();
    };
    ranges
        .iter()
        .filter_map(|(attribute, span)| {
            Some(LotLine {
                attribute: *attribute,
                value: *record.lines.get(attribute.key())?,
                span: *span,
            })
        })
        .collect()
}

/// The line a deposit is best in: the one highest in its type's span, the
/// earlier in its class's order on a tie. Purity at nought for a deposit
/// of a type `set` has not got, which no query gives.
#[must_use]
pub fn best_line(set: &ResourceSet, deposit: &Deposit) -> (Attribute, u16) {
    let mut best: Option<(f64, Attribute, u16)> = None;
    for (attribute, value) in &deposit.figures {
        let share = set
            .get(deposit.ty)
            .and_then(|def| def.ranges.get(attribute))
            .map_or(0.0, |span| span.share(*value));
        if best.is_none_or(|(held, ..)| share > held) {
            best = Some((share, *attribute, *value));
        }
    }
    best.map_or((Attribute::Purity, 0), |(_, attribute, value)| {
        (attribute, value)
    })
}

/// Which way a place lies, in eight: `+z` is north and `-x` east. The
/// world is right-handed with `+y` up, so seen from above with north at
/// the top, `+x` lies to the west: a player facing north has `-x` on their
/// right hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Compass {
    /// `+z`.
    North,
    /// Between north and east.
    NorthEast,
    /// `-x`.
    East,
    /// Between east and south.
    SouthEast,
    /// `-z`.
    South,
    /// Between south and west.
    SouthWest,
    /// `+x`.
    West,
    /// Between west and north.
    NorthWest,
}

impl Compass {
    /// Every bearing, clockwise from north.
    pub const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];

    /// The bearing's stable key (`north_east`), which a string table keys
    /// its words by.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::NorthEast => "north_east",
            Self::East => "east",
            Self::SouthEast => "south_east",
            Self::South => "south",
            Self::SouthWest => "south_west",
            Self::West => "west",
            Self::NorthWest => "north_west",
        }
    }

    /// The bearing of `to` seen from `from`, both `(x, z)`: the eighth of
    /// the compass the line falls in. North for a place underfoot.
    #[must_use]
    pub fn of(from: [f64; 2], to: [f64; 2]) -> Self {
        // The tangent of half an eighth of a turn: within it of an axis is
        // that axis, the rest a diagonal.
        const HALF_EIGHTH: f64 = 0.414_213_562_373_095_03;
        let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
        let (ax, az) = (dx.abs(), dz.abs());
        if ax <= az * HALF_EIGHTH {
            if dz >= 0.0 { Self::North } else { Self::South }
        } else if az <= ax * HALF_EIGHTH {
            if dx < 0.0 { Self::East } else { Self::West }
        } else {
            match (dx < 0.0, dz >= 0.0) {
                (true, true) => Self::NorthEast,
                (true, false) => Self::SouthEast,
                (false, false) => Self::SouthWest,
                (false, true) => Self::NorthWest,
            }
        }
    }
}

/// One row of the survey register.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterRow {
    /// The deposit.
    pub deposit: Deposit,
    /// Metres from the player, over the ground.
    pub distance_m: f64,
    /// Which way it lies.
    pub bearing: Compass,
    /// The line it is best in ([`best_line`]); a screen shows it by
    /// `reading`: not at all at Type, as a word at Bands.
    pub best: (Attribute, u16),
    /// How well the household reads it.
    pub reading: Reading,
    /// Whether the household has sampled it.
    pub sampled: bool,
    /// Whether it is the one tracked.
    pub tracked: bool,
}

/// The survey register as [`register`] lists it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Register {
    /// The rows listed, nearest first.
    pub rows: Vec<RegisterRow>,
    /// How many found deposits the range holds, of the class asked for:
    /// more than the rows when the list was cut to the dials' most.
    pub in_range: usize,
}

/// One deposit as the register words it from `from`: how far, which way,
/// its best line, what is known of it and whether it is tracked. A deposit
/// the household has not found reads at Type, which only the tracked one
/// can be asked for.
#[must_use]
pub fn register_row(
    set: &ResourceSet,
    surveyed: &Surveyed,
    deposit: &Deposit,
    from: [f64; 2],
) -> RegisterRow {
    let found = surveyed.found.get(&deposit.id);
    RegisterRow {
        distance_m: deposit.distance_m(from[0], from[1]),
        bearing: Compass::of(from, deposit.at),
        best: best_line(set, deposit),
        reading: found.map_or(Reading::Type, |found| found.reading),
        sampled: found.is_some_and(|found| found.sampled),
        tracked: surveyed.tracked.as_ref().map(|held| held.id) == Some(deposit.id),
        deposit: deposit.clone(),
    }
}

/// The survey register seen from `from`: every deposit the household has
/// found, within the register's range, of `class` when one is named,
/// nearest first, the dials' most rows at most. It lists what has been
/// found and nothing else, each with what is known of it.
///
/// The tracked deposit is always listed when it is of the class, as the
/// last row when the nearest rows do not reach it or it lies past the
/// range: its row is where it is let go, and a register that does not show
/// what the HUD points to would be wrong about the household.
#[must_use]
pub fn register(
    near: &NearDeposits,
    set: &ResourceSet,
    surveyed: &Surveyed,
    from: [f64; 2],
    class: Option<&Namespaced>,
) -> Register {
    let rules = set.rules().register;
    let listed = |deposit: &Deposit| {
        set.get(deposit.ty).is_some()
            && class.is_none_or(|class| set.class_of(deposit.ty).id == *class)
    };
    let row = |deposit: &Deposit| register_row(set, surveyed, deposit, from);
    let mut rows: Vec<RegisterRow> = near
        .deposits
        .iter()
        .filter(|deposit| surveyed.found.contains_key(&deposit.id) && listed(deposit))
        .map(row)
        .filter(|row| row.distance_m <= rules.range_m)
        .collect();
    rows.sort_by(|a, b| {
        a.distance_m
            .total_cmp(&b.distance_m)
            .then_with(|| a.deposit.id.cmp(&b.deposit.id))
    });
    let in_range = rows.len();
    let most = rules.rows as usize;
    if let Some(kept) = surveyed.tracked.as_ref().filter(|deposit| listed(deposit))
        && !rows.iter().take(most).any(|row| row.tracked)
    {
        rows.truncate(most.saturating_sub(1));
        rows.push(row(kept));
    } else {
        rows.truncate(most);
    }
    Register { rows, in_range }
}

/// Every deposit within `radius_m` of any of `around`, each once, ordered
/// by id. Pure in its inputs.
#[must_use]
pub fn near_deposits(field: &DepositField, around: &[[f64; 2]], radius_m: f64) -> Vec<Deposit> {
    let mut found: BTreeMap<DepositId, Deposit> = BTreeMap::new();
    for &[x, z] in around {
        for deposit in field.near(x, z, radius_m) {
            found.entry(deposit.id).or_insert(deposit);
        }
    }
    found.into_values().collect()
}

/// Every deposit whose area holds `(x, z)`, strongest first, each with its
/// concentration there in per-mille. A pass to
/// [`DepositField::covering`], for what uses the survey rather than the
/// field.
#[must_use]
pub fn concentration_here(survey: &Survey, x: f64, z: f64) -> Vec<(Deposit, u16)> {
    survey.0.covering(x, z)
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

    use crate::items;

    use crate::items::Pack;

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use strata_registry::Attribute;
    use strata_registry::Namespaced;
    use strata_registry::Reading;
    use strata_registry::ResourceSet;
    use strata_registry::ResourceTypeId;
    use strata_registry::STORE_GRADE;

    use strata_registry::item::Record;
    use strata_registry::item::SavedPack;

    use strata_types::Registries;

    pub fn shipped() -> Registries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        Registries(Arc::new(
            strata_registry::load_from_dir(assets).expect("assets/ loads"),
        ))
    }

    pub fn id(name: &str) -> Namespaced {
        name.parse().unwrap()
    }

    pub fn type_of(set: &ResourceSet, name: &str) -> ResourceTypeId {
        set.type_id(&id(name)).unwrap()
    }

    /// A deposit of `ty` in cell `cell`, at `at`, each figure a little above
    /// its floor: what a query would give, made by hand so no world need be
    /// baked.
    pub fn deposit(set: &ResourceSet, ty: &str, cell: [i32; 2], at: [f64; 2]) -> Deposit {
        let ty = type_of(set, ty);
        let def = set.get(ty).unwrap();
        let roots = &set.rules().roots;
        let root = roots
            [(cell[0].unsigned_abs() as usize * 7 + cell[1].unsigned_abs() as usize) % roots.len()]
        .clone();
        Deposit {
            id: DepositId(cell),
            ty,
            place: format!("{root} {}", def.grounds[0]),
            root,
            at,
            height_m: 12.0,
            figures: set
                .class_of(ty)
                .carries
                .iter()
                .enumerate()
                .map(|(index, attribute)| {
                    let span = def.ranges[attribute];
                    let over = u16::try_from(40 * (index + 1)).unwrap()
                        + u16::try_from(cell[0].unsigned_abs() % 50).unwrap();
                    (*attribute, (span.floor + over).min(span.cap))
                })
                .collect(),
            radius_m: 200.0,
            peak: 800,
            grade_pm: 500,
        }
    }

    pub const SALT: &str = "strata:evaporite_salt";

    pub const ALLOY: &str = "strata:light_alloy_ore";

    pub fn empty_pack(registries: &Registries) -> Pack {
        Pack::from_saved(&SavedPack::default(), &registries.0.items).unwrap()
    }

    /// A lot through a stack and back is the lot: the units, the figures
    /// and the deposit, with the origin in the patch and nothing else new.
    #[test]
    fn a_lot_round_trips_its_patch() {
        let registries = shipped();
        let (items, set) = (&registries.0.items, &registries.0.resources);
        let calder = deposit(set, SALT, [3, -2], [2_600.0, -1_300.0]);
        let record = lot(&calder, set, 80).unwrap();
        assert_eq!(record.item, id("strata:lot.evaporite_salt"));
        assert_eq!(record.count, 80);
        assert_eq!(record.lines.len(), 3);
        for (attribute, value) in &calder.figures {
            assert_eq!(record.lines[attribute.key()], *value);
        }
        let origin = record.origin.clone().unwrap();
        assert_eq!(origin.deposit, [3, -2]);
        assert_eq!(origin.root, calder.root);
        assert_eq!(origin.place, calder.place);
        assert_eq!(items.check_record(&record), Ok(()));

        let stack = items::write(&record, items).unwrap();
        assert_eq!(stack.count, 80);
        let keys = items.keys();
        assert!(stack.patch.get(keys.origin).is_some());
        assert!(stack.patch.get(keys.by).is_none() && stack.patch.get(keys.fitted).is_none());
        assert_eq!(items::read(&stack, items).unwrap(), record);

        // Commodity stock writes no origin at all and reads back without.
        let mut commodity = record.clone();
        commodity.origin = None;
        for value in commodity.lines.values_mut() {
            *value = STORE_GRADE;
        }
        let stack = items::write(&commodity, items).unwrap();
        assert!(stack.patch.get(keys.origin).is_none());
        assert_eq!(items::read(&stack, items).unwrap(), commodity);
        // A patch the writer did not write is refused by its key.
        let mut bent = items::write(&record, items).unwrap();
        bent.patch
            .insert(keys.origin, slotted_model::Value::Str("Calder".to_owned()));
        assert!(matches!(
            items::read(&bent, items),
            Err(items::RecordError::Patch {
                key: "strata:origin",
                ..
            })
        ));

        // The card's lines are the class's, each with its type's span, and
        // the pip says where the lot stands in them.
        let lines = lot_lines(items, &record);
        let attributes: Vec<Attribute> = lines.iter().map(|line| line.attribute).collect();
        assert_eq!(
            attributes,
            [Attribute::Purity, Attribute::Potency, Attribute::Stability]
        );
        let potency = lines[1];
        assert_eq!(
            (potency.span.floor, potency.span.cap, potency.value),
            (300, 950, calder.figure(Attribute::Potency).unwrap())
        );
        assert!(lot_lines(items, &registries.0.items.starting().unwrap().hands[0]).is_empty());
        let grade = items::grade_of(items, &record);
        let shares: f64 = lines.iter().map(|l| l.span.share(l.value)).sum::<f64>() / 3.0;
        assert!((f64::from(grade) - shares * 1000.0).abs() <= 0.5, "{grade}");
        assert_eq!(items::grade_of(items, &commodity), STORE_GRADE);
        assert_eq!(
            items::grade_of(items, &registries.0.items.starting().unwrap().hands[0]),
            STORE_GRADE
        );
    }

    /// A lot saved while a lot named its terms still reads, without them,
    /// and two lots of one deposit that differ only by an old terms string
    /// are one kind of stack after a read and a write.
    #[test]
    fn an_old_terms_key_is_read_past() {
        let registries = shipped();
        let (items, set) = (&registries.0.items, &registries.0.resources);
        let calder = deposit(set, SALT, [3, -2], [0.0, 0.0]);
        let keys = items.keys();
        let record = lot(&calder, set, 50).unwrap();
        let old = |terms: &str| {
            let mut stack = items::write(&record, items).unwrap();
            let Some(slotted_model::Value::Map(map)) = stack.patch.get(keys.origin).cloned() else {
                panic!("an origin");
            };
            let mut map = map;
            assert!(!map.contains_key("terms"), "none is written");
            map.insert(
                "terms".to_owned(),
                slotted_model::Value::Str(terms.to_owned()),
            );
            stack
                .patch
                .insert(keys.origin, slotted_model::Value::Map(map));
            stack
        };
        let (a, b) = (old("Pelagic concession"), old("Borough commons"));
        let (read_a, read_b) = (
            items::read(&a, items).unwrap(),
            items::read(&b, items).unwrap(),
        );
        assert_eq!(read_a, record);
        assert_eq!(read_b, record);
        let (again_a, again_b) = (
            items::write(&read_a, items).unwrap(),
            items::write(&read_b, items).unwrap(),
        );
        assert!(again_a.same_kind(&again_b));
        // An origin missing a word it needs is still refused.
        let mut bent = items::write(&record, items).unwrap();
        bent.patch.insert(
            keys.origin,
            slotted_model::Value::Map(BTreeMap::from([(
                "terms".to_owned(),
                slotted_model::Value::Str("x".to_owned()),
            )])),
        );
        assert!(items::read(&bent, items).is_err());
    }

    /// The deposits whose areas hold a point are the field's, strongest
    /// first, at the concentration each has there.
    #[test]
    fn concentration_here_asks_the_field() {
        let registries = shipped();
        let world = Arc::new(strata_worldgen::ProceduralWorld::with_params(
            strata_core::Seed(42),
            &registries.0,
            strata_worldgen::WorldParams::default(),
        ));
        let city = world.sites()[0].centre;
        let field = DepositField::new(world, strata_core::Seed(42), Arc::clone(&registries.0));
        let nearest = field.near(city[0], city[1], 2_000.0)[0].clone();
        let survey = Survey(Arc::new(field));
        let [x, z] = nearest.at;
        let here = concentration_here(&survey, x, z);
        assert_eq!(here, survey.0.covering(x, z));
        assert!(
            here.iter()
                .any(|(d, c)| d.id == nearest.id && *c == nearest.peak)
        );
        assert!(here.windows(2).all(|pair| pair[0].1 >= pair[1].1));
    }

    /// Two lots of one type from two deposits are two kinds of stack: put
    /// away, each takes a cell of its own.
    #[test]
    fn lots_of_two_deposits_never_merge() {
        let registries = shipped();
        let (items, set) = (&registries.0.items, &registries.0.resources);
        let calder = deposit(set, SALT, [3, -2], [0.0, 0.0]);
        let harrow = deposit(set, SALT, [4, -2], [800.0, 0.0]);
        let a = items::write(&lot(&calder, set, 50).unwrap(), items).unwrap();
        let b = items::write(&lot(&harrow, set, 50).unwrap(), items).unwrap();
        assert_eq!(a.id, b.id, "one item");
        assert!(!a.same_kind(&b), "two deposits are two kinds of stack");

        let mut pack = empty_pack(&registries);
        pack = with_lot(items, &pack, &lot(&calder, set, 50).unwrap()).unwrap();
        pack = with_lot(items, &pack, &lot(&harrow, set, 50).unwrap()).unwrap();
        let cells: Vec<Record> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .map(|stack| items::read(stack, items).unwrap())
            .collect();
        assert_eq!(cells.len(), 2);
        assert_eq!((cells[0].count, cells[1].count), (50, 50));
        assert_ne!(cells[0].origin, cells[1].origin);
        // Two deposits with the same figures still do not merge: the
        // deposit is in the patch, not only its numbers.
        let mut twin = harrow.clone();
        twin.figures.clone_from(&calder.figures);
        let c = items::write(&lot(&twin, set, 50).unwrap(), items).unwrap();
        assert!(!a.same_kind(&c));
        // Nor does a lot with commodity stock of its type.
        let mut commodity = lot(&calder, set, 50).unwrap();
        commodity.origin = None;
        for value in commodity.lines.values_mut() {
            *value = STORE_GRADE;
        }
        assert!(!a.same_kind(&items::write(&commodity, items).unwrap()));
    }

    /// Two lots of one deposit are one kind of stack: the second tops up
    /// the first to the stack's limit and the rest takes the next cell.
    #[test]
    fn of_one_deposit_do() {
        let registries = shipped();
        let (items, set) = (&registries.0.items, &registries.0.resources);
        let calder = deposit(set, SALT, [3, -2], [0.0, 0.0]);
        let mut pack = empty_pack(&registries);
        for units in [50, 50, 200] {
            pack = with_lot(items, &pack, &lot(&calder, set, units).unwrap()).unwrap();
        }
        let counts: Vec<u32> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .map(|stack| stack.count)
            .collect();
        assert_eq!(counts, [200, 100], "300 units of one deposit in two cells");
        let first = items::read(pack.cells.get(0).unwrap(), items).unwrap();
        assert_eq!(first.origin, lot(&calder, set, 1).unwrap().origin);
    }

    /// A household that has found every deposit of `near`, read only by
    /// type, apart from the ones `sampled` names.
    pub fn found_all(near: &NearDeposits, sampled: &[DepositId]) -> Surveyed {
        Surveyed {
            found: near
                .deposits
                .iter()
                .map(|deposit| {
                    let sampled = sampled.contains(&deposit.id);
                    (
                        deposit.id,
                        Found {
                            reading: if sampled {
                                Reading::Exact
                            } else {
                                Reading::Type
                            },
                            sampled,
                            best_pm: deposit.peak,
                            best_at: deposit.at,
                        },
                    )
                })
                .collect(),
            ..Surveyed::default()
        }
    }

    /// The register lists what the household has found, nearest first, with
    /// the eighth of the compass each lies in, what is known of each and a
    /// mark on the one tracked; a deposit not found is not listed; a class
    /// narrows it and the dials cap it.
    #[test]
    fn the_register_is_nearest_first_with_bearings() {
        let registries = shipped();
        let set = &registries.0.resources;
        let from = [100.0, -50.0];
        // One deposit each way, further out the further round the compass.
        let ways: [(Compass, [f64; 2]); 8] = [
            (Compass::North, [0.0, 1.0]),
            (Compass::NorthEast, [-0.7, 0.7]),
            (Compass::East, [-1.0, 0.05]),
            (Compass::SouthEast, [-0.6, -0.8]),
            (Compass::South, [0.1, -1.0]),
            (Compass::SouthWest, [0.7, -0.7]),
            (Compass::West, [1.0, 0.2]),
            (Compass::NorthWest, [0.7, 0.72]),
        ];
        assert_eq!(ways.map(|(way, _)| way), Compass::ALL);
        let mut deposits = Vec::new();
        for (n, (_, [dx, dz])) in ways.iter().enumerate() {
            let out = 500.0 * f64::from(u32::try_from(n).unwrap() + 1);
            let ty = if n % 2 == 0 { SALT } else { ALLOY };
            let cell = [i32::try_from(n).unwrap(), 0];
            deposits.push(deposit(
                set,
                ty,
                cell,
                [from[0] + dx * out, from[1] + dz * out],
            ));
        }
        // One past the register's range, which it leaves out.
        let far = deposit(set, SALT, [40, 40], [from[0] + 17_000.0, from[1]]);
        deposits.push(far.clone());
        // One near, not found by anyone, which the register never lists.
        let unfound = deposit(set, SALT, [41, 41], [from[0] + 10.0, from[1]]);
        deposits.push(unfound.clone());
        // Farthest first: the register does the sorting, whatever order
        // the list is kept in.
        deposits.reverse();
        let near = NearDeposits {
            deposits: deposits.clone(),
            around: vec![from],
        };
        let mut surveyed = found_all(&near, &[DepositId([2, 0])]);
        surveyed.found.remove(&unfound.id);
        surveyed.tracked = Some(near.get(DepositId([5, 0])).unwrap().clone());

        let listed = register(&near, set, &surveyed, from, None);
        assert_eq!(listed.in_range, 8, "the one 17 km off is out of range");
        let rows = listed.rows;
        assert_eq!(rows.len(), 8);
        let mut last = 0.0;
        for (n, row) in rows.iter().enumerate() {
            let n32 = i32::try_from(n).unwrap();
            assert_eq!(row.deposit.id, DepositId([n32, 0]), "nearest first");
            assert!(row.distance_m > last);
            last = row.distance_m;
            assert_eq!(row.bearing, ways[n].0, "{n}");
            assert_eq!(row.sampled, n == 2);
            assert_eq!(
                row.reading,
                if n == 2 {
                    Reading::Exact
                } else {
                    Reading::Type
                }
            );
            assert_eq!(row.tracked, n == 5);
            assert_eq!(row.best, best_line(set, &row.deposit));
        }
        assert!((rows[0].distance_m - 500.0).abs() < 1e-6);
        assert!((rows[1].distance_m - 1_000.0 * (0.98_f64).sqrt()).abs() < 1e-6);
        // The best line is the one highest in its span, not the largest
        // number: salt's figures here rise 40 a line over floors that fall.
        let (attribute, value) = rows[0].best;
        assert_eq!(attribute, Attribute::Stability);
        assert_eq!(value, rows[0].deposit.figure(Attribute::Stability).unwrap());
        assert_eq!(Compass::NorthEast.key(), "north_east");
        assert_eq!(Compass::of(from, from), Compass::North);

        // By class: the reactive ones are the salt, four of the eight.
        let reactive = id("strata:reactive");
        let rows = register(&near, set, &surveyed, from, Some(&reactive)).rows;
        let ids: Vec<i32> = rows.iter().map(|row| row.deposit.id.0[0]).collect();
        assert_eq!(ids, [0, 2, 4, 6]);
        let brine = register(&near, set, &surveyed, from, Some(&id("strata:brine")));
        assert!(brine.rows.is_empty() && brine.in_range == 0);

        // The tracked deposit past the range is still listed, last, since
        // its row is where it is let go; and only under its own class.
        surveyed.tracked = Some(far.clone());
        let listed = register(&near, set, &surveyed, from, None);
        assert_eq!(listed.in_range, 8, "it is not counted as in range");
        assert_eq!(listed.rows.len(), 9);
        let last = listed.rows.last().unwrap();
        assert!(last.tracked && last.deposit.id == far.id);
        assert!(last.distance_m > 16_000.0 && last.bearing == Compass::West);
        assert_eq!(listed.rows.iter().filter(|row| row.tracked).count(), 1);
        let alloy = register(
            &near,
            set,
            &surveyed,
            from,
            Some(&id("strata:structural_metal")),
        );
        assert!(alloy.rows.iter().all(|row| !row.tracked));
    }

    /// The dials cap the list at the nearest rows and say how many the
    /// range holds; the tracked deposit keeps a row however far down the
    /// list it would be.
    #[test]
    fn the_register_is_capped_and_keeps_the_tracked_row() {
        let registries = shipped();
        let set = &registries.0.resources;
        let from = [0.0, 0.0];
        // The dials' cap: sixty rows of a hundred deposits, the nearest.
        let many: Vec<Deposit> = (0..100)
            .map(|n| {
                deposit(
                    set,
                    SALT,
                    [n, 1],
                    [from[0] + 50.0 * f64::from(n + 1), from[1]],
                )
            })
            .collect();
        let near = NearDeposits {
            deposits: many,
            around: vec![from],
        };
        let listed = register(&near, set, &found_all(&near, &[]), from, None);
        assert_eq!(listed.in_range, 100, "what the caption counts");
        let rows = listed.rows;
        assert_eq!(rows.len(), 60);
        assert_eq!(rows[59].deposit.id, DepositId([59, 1]));
        assert!(rows.iter().all(|row| row.bearing == Compass::West));

        // A tracked deposit the nearest sixty do not reach keeps its row:
        // the list is still sixty, the nearest fifty-nine and it, and it is
        // there once. One inside the sixty is where it lies, not moved.
        let beyond = near.get(DepositId([80, 1])).unwrap().clone();
        let surveyed = Surveyed {
            tracked: Some(beyond.clone()),
            ..found_all(&near, &[])
        };
        let listed = register(&near, set, &surveyed, from, None);
        assert_eq!((listed.rows.len(), listed.in_range), (60, 100));
        assert_eq!(listed.rows[58].deposit.id, DepositId([58, 1]));
        assert_eq!(listed.rows[59].deposit.id, beyond.id);
        assert!(listed.rows[59].tracked);
        assert_eq!(listed.rows.iter().filter(|row| row.tracked).count(), 1);
        let within = Surveyed {
            tracked: Some(near.get(DepositId([10, 1])).unwrap().clone()),
            ..found_all(&near, &[])
        };
        let listed = register(&near, set, &within, from, None);
        assert_eq!(listed.rows.len(), 60);
        assert!(listed.rows[10].tracked);
        assert_eq!(listed.rows[59].deposit.id, DepositId([59, 1]));
    }

    /// The place the HUD names: the nearest deposit within the dials'
    /// `place_m`, and none from further off.
    #[test]
    fn the_deposit_stood_at_is_the_nearest_within_the_dial() {
        let registries = shipped();
        let set = &registries.0.resources;
        let within = set.rules().place.place_m;
        let calder = deposit(set, SALT, [3, -2], [0.0, 0.0]);
        let harrow = deposit(set, ALLOY, [4, -2], [within, 0.0]);
        let near = NearDeposits {
            deposits: vec![harrow.clone(), calder.clone()],
            around: vec![[0.0, 0.0]],
        };
        let at = |x: f64| standing_at(&near, set, [x, 0.0]).map(|deposit| deposit.id);
        assert_eq!(at(0.0), Some(calder.id));
        assert_eq!(
            at(-within),
            Some(calder.id),
            "the dial's distance is within"
        );
        assert_eq!(at(-within - 0.5), None);
        assert_eq!(at(within * 0.75), Some(harrow.id), "the nearer of two");
        assert_eq!(at(within * 0.5), Some(calder.id), "the lower id on a tie");
        assert_eq!(standing_at(&NearDeposits::default(), set, [0.0, 0.0]), None);
    }
}
