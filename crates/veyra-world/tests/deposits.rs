//! Deposits on seed 42 (ticket 0007, step 3): the same by id and by query,
//! on one thread and on eight and on a second call; never in water, on a
//! road or on a pad; every figure inside its type's floor and cap and none
//! for an attribute the class does not carry; two of one class differing;
//! every type within a walk of Port Serein; a name not repeated within its
//! block; and none of it in the world's version.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rayon::prelude::*;
use strata_core::Seed;
use strata_worldgen::{WorldParams, WorldSource, world_version};
use veyra_data::{Attribute, ItemCatalogue, ResourceSet};
use veyra_world::{Deposit, DepositField, DepositId};

/// The shipped world's deposits, its rivers baked as [`common::world`]
/// bakes them.
fn shipped() -> DepositField {
    let world = Arc::new(common::world(42));
    DepositField::new(world, Seed(42), Arc::new(common::registries()))
}

fn city(field: &DepositField) -> [f64; 2] {
    field.world().sites()[0].centre
}

/// Every deposit within eight kilometres of Port Serein.
fn round_the_city(field: &DepositField) -> Vec<Deposit> {
    let [x, z] = city(field);
    let near = field.near(x, z, 8_000.0);
    assert!(near.len() > 60, "only {} deposits within 8 km", near.len());
    near
}

fn id(name: &str) -> strata_data::Namespaced {
    name.parse().expect("a valid id")
}

/// Where a figure stands in its type's span, nought to one.
fn share(set: &ResourceSet, deposit: &Deposit, attribute: Attribute, value: u16) -> f64 {
    set.get(deposit.ty).expect("its own type").ranges[&attribute].share(value)
}

/// The resources are in no version: the world with them and the world with
/// none (and so with no lot items) hash alike.
#[test]
fn the_world_version_ignores_resources() {
    let registries = common::registries();
    assert_eq!(registries.resources.types().len(), 7, "the resources ship");
    let params = WorldParams::default();
    let with = world_version(&params, &registries);
    let mut bare = registries.clone();
    bare.resources = ResourceSet::default();
    bare.items = ItemCatalogue::default();
    assert_ne!(bare, registries);
    assert_eq!(with, world_version(&params, &bare));
    // And a world without them holds no deposit at all.
    let world = Arc::new(common::world(42));
    let empty = DepositField::new(world, Seed(42), Arc::new(bare));
    let [x, z] = city(&empty);
    assert!(empty.near(x, z, 4_000.0).is_empty());
    assert_eq!(empty.get(DepositId([0, 0])), None);
}

/// A query lists its deposits nearest first, each once, each in the cell
/// its id names; asking for one by id gives the same deposit, as does
/// asking again, and a wider query holds the narrower one.
#[test]
fn a_deposit_is_the_same_by_id_and_by_query() {
    let field = shipped();
    let [x, z] = city(&field);
    let near = field.near(x, z, 6_000.0);
    assert!(near.len() > 30, "only {} within 6 km", near.len());
    let mut seen = BTreeSet::new();
    let mut last = 0.0;
    for deposit in &near {
        assert!(seen.insert(deposit.id), "{} twice", deposit.id);
        let distance = deposit.distance_m(x, z);
        assert!(distance >= last && distance <= 6_000.0, "{distance}");
        last = distance;
        assert_eq!(field.cell_of(deposit.at[0], deposit.at[1]), deposit.id);
        assert_eq!(field.get(deposit.id).as_ref(), Some(deposit));
        assert!(!deposit.root.is_empty() && deposit.place.starts_with(&deposit.root));
        let set = field.set();
        let def = set.get(deposit.ty).expect("its own type");
        assert_eq!(
            deposit.lot_name(set),
            format!("{} {}", deposit.root, def.short)
        );
        let ground = deposit.place.strip_prefix(&deposit.root).expect("checked");
        assert!(
            def.grounds.iter().any(|word| ground.trim() == word),
            "{}",
            deposit.place
        );
    }
    assert_eq!(field.near(x, z, 6_000.0), near);
    let wider = field.near(x, z, 8_000.0);
    assert_eq!(wider[..near.len()], near[..]);
    assert_eq!(
        field.nearest(x, z, 6_000.0, |_| true).as_ref(),
        near.first()
    );
    let far = near.last().expect("some");
    assert_eq!(
        field.nearest(x, z, 6_000.0, |d| d.id == far.id).as_ref(),
        Some(far)
    );
    // A cell that holds none says so by id too.
    let lattice = field.set().rules().lattice;
    let cells = |metres: f64| field.cell_of(metres, 0.0).0[0];
    let empty = (cells(x - 6_000.0)..=cells(x + 6_000.0))
        .flat_map(|cx| {
            (field.cell_of(0.0, z - 6_000.0).0[1]..=field.cell_of(0.0, z + 6_000.0).0[1])
                .map(move |cz| DepositId([cx, cz]))
        })
        .filter(|id| field.get(*id).is_none())
        .count();
    assert!(empty > 0, "the sea and the dropped cells hold none");
    assert!(lattice.cell_m > 0.0);
}

/// Every cell within six kilometres of Port Serein, asked for by id on one
/// thread and on a pool of eight, is the same deposit or the same nothing.
#[test]
fn deposits_are_the_same_on_one_thread_and_eight() {
    let field = shipped();
    let [x, z] = city(&field);
    let (lo, hi) = (
        field.cell_of(x - 6_000.0, z - 6_000.0).0,
        field.cell_of(x + 6_000.0, z + 6_000.0).0,
    );
    let ids: Vec<DepositId> = (lo[1]..=hi[1])
        .flat_map(|cz| (lo[0]..=hi[0]).map(move |cx| DepositId([cx, cz])))
        .collect();
    let one: Vec<Option<Deposit>> = ids.iter().map(|id| field.get(*id)).collect();
    let eight: Vec<Option<Deposit>> = {
        let _one_pool = common::one_sweep_at_a_time();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .expect("a thread pool");
        pool.install(|| ids.par_iter().map(|id| field.get(*id)).collect())
    };
    assert!(one.iter().flatten().count() > 30);
    assert_eq!(one, eight);
    // And a query made on the pool is the one made here.
    let here = field.near(x, z, 6_000.0);
    let there: Vec<Vec<Deposit>> = {
        let _one_pool = common::one_sweep_at_a_time();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .expect("a thread pool");
        pool.install(|| {
            (0..8)
                .into_par_iter()
                .map(|_| field.near(x, z, 6_000.0))
                .collect()
        })
    };
    for list in there {
        assert_eq!(list, here);
    }
    // A second field over the same inputs is the same field.
    let again = shipped();
    assert_eq!(again.near(x, z, 6_000.0), here);
}

/// Sampled again where it stands, no deposit within eight kilometres is in
/// a river or a lake, at the sea's edge, on steep ground, on a road's
/// cleared way or carriageway, or on a pad or a settlement's ground.
#[test]
fn no_deposit_stands_in_water_on_a_road_or_on_a_pad() {
    let field = shipped();
    let world = field.world();
    let refuse = field.set().rules().refuse;
    let sea = f64::from(world.sea_level());
    for deposit in round_the_city(&field) {
        let [x, z] = deposit.at;
        let sample = world.sample(x, z);
        assert_eq!(sample.height.to_bits(), deposit.height_m.to_bits());
        assert!(!sample.in_water(), "{} is in water", deposit.place);
        assert!(sample.standing_water().is_none(), "{}", deposit.place);
        assert!(
            f64::from(sample.height) >= sea + refuse.shore_m,
            "{} at the sea's edge: {}",
            deposit.place,
            sample.height
        );
        assert!(f64::from(sample.landform.slope) <= refuse.slope_max);
        assert!(
            f64::from(sample.landform.cleared) <= refuse.cleared_max,
            "{} on a road's way",
            deposit.place
        );
        assert!(
            f64::from(sample.landform.developed) <= refuse.developed_max,
            "{} on laid ground",
            deposit.place
        );
        for site in world.sites_near(x, z, 500.0) {
            let (dx, dz) = (site.centre[0] - x, site.centre[1] - z);
            assert!(
                (dx * dx + dz * dz).sqrt() > site.radius_m,
                "{} on {}'s pad",
                deposit.place,
                site.name
            );
        }
        if let Some(road) = world.nearest_road(x, z, 60.0) {
            let half = strata_worldgen::roads::spec(road.class).half_width_m;
            assert!(
                road.distance_m > half,
                "{} is {:.1} m from a road's centreline",
                deposit.place,
                road.distance_m
            );
        }
        for site in world.sites() {
            assert!(
                site.extent_distance_at(x, z) > 0.0,
                "{} inside {}",
                deposit.place,
                site.id
            );
        }
    }
}

/// A deposit has one figure for each attribute its class carries, in the
/// class's order, each inside its type's floor and cap, and none for an
/// attribute its class does not carry.
#[test]
fn figures_stay_inside_their_type() {
    let field = shipped();
    let set = field.set();
    let mut types = BTreeSet::new();
    for deposit in round_the_city(&field) {
        let def = set.get(deposit.ty).expect("its own type");
        let class = set.class_of(deposit.ty);
        let carried: Vec<Attribute> = deposit.figures.iter().map(|(a, _)| *a).collect();
        assert_eq!(carried, class.carries, "{}", deposit.place);
        for (attribute, value) in &deposit.figures {
            let span = def.ranges[attribute];
            assert!(
                span.contains(*value),
                "{}: {attribute:?} {value} outside {} to {}",
                deposit.place,
                span.floor,
                span.cap
            );
        }
        for attribute in Attribute::ALL {
            assert_eq!(
                deposit.figure(attribute).is_some(),
                class.carries.contains(&attribute),
                "{}: {attribute:?}",
                deposit.place
            );
        }
        types.insert(deposit.ty);
    }
    assert_eq!(types.len(), set.types().len(), "every type was looked at");
}

/// One shared roll and each attribute's own make a deposit spiky: in most
/// of them the best line stands well over the rest in its span, and across
/// them the best line is not always the same one.
#[test]
fn most_deposits_are_spiky() {
    let field = shipped();
    let set = field.set();
    let deposits = round_the_city(&field);
    let mut spiky = 0;
    let mut best_lines = BTreeSet::new();
    let mut gaps = Vec::new();
    for deposit in &deposits {
        let mut shares: Vec<(f64, Attribute)> = deposit
            .figures
            .iter()
            .map(|(attribute, value)| (share(set, deposit, *attribute, *value), *attribute))
            .collect();
        shares.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, line) = shares[0];
        // Counts of a handful of lines.
        #[allow(clippy::cast_precision_loss)]
        let rest = shares[1..].iter().map(|(s, _)| s).sum::<f64>() / (shares.len() - 1) as f64;
        gaps.push(best - rest);
        if best - rest >= 0.2 {
            spiky += 1;
        }
        best_lines.insert((deposit.ty, line));
    }
    gaps.sort_by(f64::total_cmp);
    let median = gaps[gaps.len() / 2];
    assert!(
        spiky * 5 >= deposits.len() * 3,
        "{spiky} of {} deposits stand a fifth of a span over their rest (median gap {median:.2})",
        deposits.len()
    );
    assert!(median >= 0.25, "the median gap is {median:.2}");
    assert!(
        best_lines.len() > set.types().len() * 2,
        "the best line is nearly always the same one: {best_lines:?}"
    );
}

/// The step's done-when, on the figures: of every class and of every type
/// with two deposits within eight kilometres, no two read the same, and
/// the nearest two of a class differ in name as well.
#[test]
fn two_deposits_of_a_class_differ() {
    let field = shipped();
    let set = field.set();
    let deposits = round_the_city(&field);
    let mut by_class: BTreeMap<usize, Vec<&Deposit>> = BTreeMap::new();
    for deposit in &deposits {
        by_class
            .entry(set.class_index(deposit.ty))
            .or_default()
            .push(deposit);
    }
    assert_eq!(by_class.len(), set.classes().len(), "every class is near");
    for (class, list) in &by_class {
        let name = &set.classes()[*class].name;
        assert!(list.len() >= 2, "only {} of {name}", list.len());
        for (i, a) in list.iter().enumerate() {
            for b in &list[i + 1..] {
                let same_type = a.ty == b.ty;
                assert!(
                    !same_type || a.figures != b.figures,
                    "{} and {} read alike: {:?}",
                    a.place,
                    b.place,
                    a.figures
                );
            }
        }
        let (a, b) = (list[0], list[1]);
        assert_ne!(a.id, b.id);
        assert_ne!(a.lot_name(set), b.lot_name(set), "{name}");
        assert_ne!(
            a.figures.iter().map(|f| f.1).collect::<Vec<_>>(),
            b.figures.iter().map(|f| f.1).collect::<Vec<_>>(),
            "{name}: {} and {}",
            a.place,
            b.place
        );
    }
    // Two deposits of the reactive class of two types: their lines are the
    // same three and their spans are not.
    let salt = set.type_id(&id("strata:evaporite_salt")).expect("ships");
    let sulphur = set.type_id(&id("strata:sulphur")).expect("ships");
    let find = |ty| deposits.iter().find(|d| d.ty == ty).expect("within 8 km");
    let (a, b) = (find(salt), find(sulphur));
    assert_eq!(set.class_index(a.ty), set.class_index(b.ty));
    assert_ne!(a.figures, b.figures);
}

/// Decision 5 of the step, held as a test with room for a re-tune: from
/// Port Serein the worked example's three (evaporite salt, a brine, light
/// alloy ore) are within five kilometres and all seven types within eight.
#[test]
fn every_type_is_near_port_serein() {
    let field = shipped();
    let set = field.set();
    let [x, z] = city(&field);
    let near = round_the_city(&field);
    let nearest = |wanted: &dyn Fn(&Deposit) -> bool| {
        near.iter()
            .find(|deposit| wanted(deposit))
            .map(|deposit| deposit.distance_m(x, z))
    };
    for ty in set.type_ids() {
        let name = &set.get(ty).expect("its own handle").name;
        assert!(
            nearest(&|d| d.ty == ty).is_some(),
            "no {name} within 8 km of Port Serein"
        );
    }
    let brine = id("strata:brine");
    let example: [(&str, Box<dyn Fn(&Deposit) -> bool>); 3] = [
        (
            "evaporite salt",
            Box::new(|d| {
                set.get(d.ty)
                    .is_some_and(|t| t.id == id("strata:evaporite_salt"))
            }),
        ),
        ("a brine", Box::new(|d| set.class_of(d.ty).id == brine)),
        (
            "light alloy ore",
            Box::new(|d| {
                set.get(d.ty)
                    .is_some_and(|t| t.id == id("strata:light_alloy_ore"))
            }),
        ),
    ];
    for (name, wanted) in &example {
        let distance = nearest(wanted.as_ref()).unwrap_or(f64::INFINITY);
        assert!(
            distance <= 5_000.0,
            "{name} is {distance:.0} m from Port Serein"
        );
    }
}

/// The roots are dealt once over a block of cells, so the same name is a
/// block away or further: sixteen cells of 400 m less what two deposits may
/// stand off their cells' centres by, which is six kilometres.
#[test]
fn a_root_does_not_repeat_within_six_kilometres() {
    let field = shipped();
    let rules = field.set().rules();
    let [x, z] = city(&field);
    let near = field.near(x, z, 8_000.0);
    assert!(near.len() > 200, "only {} within 8 km", near.len());
    // A block of names and the lattice's cell.
    #[allow(clippy::cast_precision_loss)]
    let block_m = rules.root_block() as f64 * rules.lattice.cell_m;
    let apart = block_m - rules.lattice.jitter * rules.lattice.cell_m;
    assert!(apart >= 6_000.0, "a name repeats {apart:.0} m on");
    let mut by_root: BTreeMap<&str, Vec<&Deposit>> = BTreeMap::new();
    for deposit in &near {
        by_root.entry(&deposit.root).or_default().push(deposit);
    }
    let mut repeated = 0;
    for (root, list) in &by_root {
        assert!(rules.roots.iter().any(|known| known == root), "{root}");
        for (i, a) in list.iter().enumerate() {
            for b in &list[i + 1..] {
                let d = a.distance_m(b.at[0], b.at[1]);
                assert!(d >= apart, "{root} twice, {d:.0} m apart");
                repeated += 1;
            }
        }
    }
    assert!(repeated > 0, "16 km across holds a name twice");
    assert!(by_root.len() > 100, "{} names in use", by_root.len());
}

/// What the carbine's schematic makes of a deposit's metal on its
/// moddability line: the weighted figures of its frame, per mille. `None`
/// for a deposit whose class the frame does not take.
fn platform_ceiling(registries: &veyra_data::GameRegistries, deposit: &Deposit) -> Option<u32> {
    use veyra_data::schematic::Stat;
    let carbine = registries
        .schematics
        .get(&id("veyra:schematic.patrol_carbine"))
        .expect("the carbine ships");
    let line = carbine
        .def
        .lines
        .iter()
        .find(|line| line.line == "moddability")
        .expect("a carbine has a moddability line");
    let mut sum = 0.0;
    for term in &line.from {
        let Stat::Attribute(attribute) = &term.stat else {
            panic!("the carbine's moddability reads its frame alone");
        };
        sum += f64::from(deposit.figure(*attribute)?) * f64::from(term.weight);
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a weighted mean of figures no larger than a thousand"
    )]
    Some(sum.round() as u32)
}

/// A made carbine can beat a bought one without a journey: the muzzle
/// opens at 600 on the moddability line, and metal whose ceiling passes it
/// lies within three kilometres of Port Serein and in plenty within eight.
/// The fore end's 800 is not promised near the city; it is the far find.
#[test]
fn platform_metal_is_within_a_walk_of_the_city() {
    let field = shipped();
    let registries = common::registries();
    let [x, z] = city(&field);
    let platform = |radius: f64| {
        field
            .near(x, z, radius)
            .iter()
            .filter(|deposit| platform_ceiling(&registries, deposit).is_some_and(|c| c >= 600))
            .count()
    };
    assert!(platform(3_000.0) >= 1, "none within 3 km");
    assert!(
        platform(8_000.0) >= 8,
        "only {} within 8 km",
        platform(8_000.0)
    );
}

/// A deposit's concentration is its peak at the centre, nothing at the
/// radius and beyond it, and never rises as the point moves away.
#[test]
fn concentration_falls_from_the_peak_to_nothing_at_the_radius() {
    let field = shipped();
    let deposits = round_the_city(&field);
    for deposit in &deposits {
        let [x, z] = deposit.at;
        assert_eq!(
            deposit.concentration(x, z),
            deposit.peak,
            "{}",
            deposit.place
        );
        let radius = f64::from(deposit.radius_m);
        assert_eq!(deposit.concentration(x + radius, z), 0);
        assert_eq!(deposit.concentration(x, z - radius), 0);
        assert_eq!(deposit.concentration(x + radius * 3.0, z), 0);
        let mut last = deposit.peak;
        for step in 0..=120 {
            let at = radius * 1.2 * f64::from(step) / 120.0;
            let here = deposit.concentration(x + at, z);
            assert!(here <= last, "{} rises at {at:.1} m", deposit.place);
            last = here;
        }
        assert_eq!(last, 0);
    }
}

/// What covers a point is the same on a second call and on two threads,
/// strongest first, each with the concentration it has there.
#[test]
fn covering_is_the_same_twice_and_on_two_threads() {
    let field = shipped();
    let deposits = round_the_city(&field);
    // Points on a deposit's centre and a little off it, so some are held.
    let points: Vec<[f64; 2]> = deposits
        .iter()
        .take(40)
        .flat_map(|d| [d.at, [d.at[0] + 90.0, d.at[1] - 40.0]])
        .collect();
    let first: Vec<_> = points.iter().map(|p| field.covering(p[0], p[1])).collect();
    let second: Vec<_> = points.iter().map(|p| field.covering(p[0], p[1])).collect();
    assert_eq!(first, second);
    let _one_pool = common::one_sweep_at_a_time();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("a thread pool");
    let there: Vec<_> = pool.install(|| {
        points
            .par_iter()
            .map(|p| field.covering(p[0], p[1]))
            .collect()
    });
    assert_eq!(first, there);
    for (point, list) in points.iter().zip(&first) {
        for pair in list.windows(2) {
            assert!(
                pair[0].1 > pair[1].1 || (pair[0].1 == pair[1].1 && pair[0].0.id < pair[1].0.id)
            );
        }
        for (deposit, strength) in list {
            assert!(*strength > 0);
            assert_eq!(*strength, deposit.concentration(point[0], point[1]));
        }
    }
    // A centre is covered by its own deposit at its peak.
    for (deposit, list) in deposits.iter().take(40).zip(first.iter().step_by(2)) {
        assert!(
            list.iter()
                .any(|(d, c)| d.id == deposit.id && *c == deposit.peak),
            "{}",
            deposit.place
        );
    }
}

/// Every radius and peak is inside the dials.
#[test]
fn every_radius_and_peak_is_inside_the_dials() {
    let field = shipped();
    let area = field.set().rules().area;
    let (mut radii, mut peaks) = (BTreeSet::new(), BTreeSet::new());
    for deposit in round_the_city(&field) {
        let radius = f64::from(deposit.radius_m);
        assert!(
            radius >= area.radius_m.0 && radius <= area.radius_m.1,
            "{}: {radius}",
            deposit.place
        );
        assert!(
            (area.peak.0..=area.peak.1).contains(&deposit.peak),
            "{}: {}",
            deposit.place,
            deposit.peak
        );
        radii.insert(deposit.radius_m.to_bits());
        peaks.insert(deposit.peak);
    }
    assert!(radii.len() > 50 && peaks.len() > 50, "the rolls vary");
}

/// A household starts with every class of resource a short walk from the
/// city: some deposit of each class within this many metres of the city's
/// anchor. Measured on seed 42 the nearest are Reactive 681 m, Brine 736 m,
/// Structural metal 769 m and Conductor 988 m: the city's own developed
/// ground refuses candidates, so 500 m is not reachable and 1000 m is the
/// least of 600, 800 and 1000 that holds for every class.
const STARTER_M: f64 = 1_000.0;

/// Every resource class has a deposit within [`STARTER_M`] of Port Serein,
/// by the lattice and the type weights alone.
#[test]
fn every_class_has_a_deposit_within_a_short_walk_of_the_city() {
    let field = shipped();
    let set = field.set();
    let [x, z] = city(&field);
    let near = field.near(x, z, 2_000.0);
    let mut nearest_per_class = Vec::new();
    for index in 0..set.classes().len() {
        let nearest = near
            .iter()
            .filter(|deposit| set.class_index(deposit.ty) == index)
            .map(|deposit| deposit.distance_m(x, z))
            .fold(f64::INFINITY, f64::min);
        nearest_per_class.push((index, nearest));
    }
    println!("nearest deposit per class (index, metres): {nearest_per_class:.0?}");
    assert!(
        nearest_per_class.iter().all(|&(_, m)| m <= STARTER_M),
        "a class is farther than {STARTER_M} m: {nearest_per_class:.0?}"
    );
}
