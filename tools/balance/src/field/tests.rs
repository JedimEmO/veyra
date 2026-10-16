//! The model on a fixture small enough to work by hand, and the file's
//! checks.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None or an Err fails, which is the point"
)]
#![allow(
    clippy::semicolon_if_nothing_returned,
    reason = "one-line closures that change a file, kept as expressions"
)]

use std::path::PathBuf;

use strata_registry::GameRegistries;

use super::*;

fn assets() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

fn registries() -> GameRegistries {
    strata_registry::load_from_dir(assets()).expect("the shipped assets load")
}

const HOME: [f64; 2] = [0.0, 0.0];
const FOOT: (f64, f64) = (5.5, 8.5);

/// A deposit of the named type, every figure 800, `east` metres east of
/// home, 100 m across, peaking at 800.
fn deposit(registries: &GameRegistries, ty: &str, east: f64, n: i32) -> Deposit {
    let set = &registries.resources;
    let id = set
        .type_ids()
        .find(|t| set.get(*t).is_some_and(|d| d.id.to_string() == ty))
        .expect("the type exists");
    Deposit {
        id: DepositId([n, 0]),
        ty: id,
        root: "Fixture".to_owned(),
        place: "Fixture flats".to_owned(),
        at: [east, 0.0],
        height_m: 10.0,
        figures: set.class_of(id).carries.iter().map(|a| (*a, 800)).collect(),
        radius_m: 100.0,
        peak: 800,
        grade_pm: 500,
    }
}

fn ground(registries: &GameRegistries) -> Fixed {
    Fixed(vec![
        deposit(registries, "strata:red_conductor", 1000.0, 1),
        deposit(registries, "strata:evaporite_salt", 200.0, 2),
        deposit(registries, "strata:basin_brine", 300.0, 3),
    ])
}

fn file(path: &str) -> FirstHour {
    let text = std::fs::read_to_string(assets().join(FILE)).unwrap();
    let mut file: FirstHour = ron::from_str(&text).unwrap();
    let steps: Vec<Step> = ron::from_str(path).unwrap();
    file.path = steps;
    file.bands = Vec::new();
    for step in &mut file.path {
        step.mark = None;
    }
    file
}

fn go(registries: &GameRegistries, file: &FirstHour, variant: &Variant) -> Timeline {
    run(registries, &ground(registries), file, HOME, FOOT, variant)
}

#[test]
fn the_shipped_file_loads_and_is_checked() {
    let registries = registries();
    let shipped = FirstHour::load(&assets(), &registries).expect("the file loads");
    assert_eq!(shipped.version, FirstHour::VERSION);
    assert!(!shipped.path.is_empty());
}

#[test]
fn a_file_that_contradicts_the_data_is_refused() {
    let registries = registries();
    let good = FirstHour::load(&assets(), &registries).unwrap();
    let broken: Vec<(&str, Box<dyn Fn(&mut FirstHour)>)> = vec![
        (
            "a band twice",
            Box::new(|f| f.bands.push(f.bands[0].clone())),
        ),
        (
            "an unknown item",
            Box::new(|f| {
                f.path.push(Step {
                    mark: None,
                    what: Do::Buy(Namespaced::parse("veyra:nothing.at_all").unwrap()),
                })
            }),
        ),
        (
            "an unknown box",
            Box::new(|f| {
                f.path.push(Step {
                    mark: None,
                    what: Do::Learn {
                        id: "nobody.novice".into(),
                        member: "Ilse Marrow".into(),
                    },
                })
            }),
        ),
        (
            "an unknown member",
            Box::new(|f| {
                f.path.push(Step {
                    mark: None,
                    what: Do::Take("Nobody".into()),
                })
            }),
        ),
        (
            "a negative assumption",
            Box::new(|f| f.travel.detour = -1.0),
        ),
        ("a negative walk", Box::new(|f| f.find.walked_m = -1.0)),
        (
            "a mark without a band",
            Box::new(|f| f.path[0].mark = Some("no_such_band".into())),
        ),
        (
            "a band nothing reaches",
            Box::new(|f| {
                f.bands.push(Band {
                    id: "lonely".into(),
                    by_min: 5.0,
                    opens: None,
                    waived: None,
                })
            }),
        ),
    ];
    for (what, change) in broken {
        let mut file = good.clone();
        change(&mut file);
        assert!(file.check(&registries).is_err(), "{what} should be refused");
    }
}

/// Worked by hand on the shipped dials: the arrival buys the bike, rides to
/// a conductor 1000 m east whose area is 100 m across, finds it, samples
/// fifteen units and makes a harness.
///
/// Bike: ground 36 m/s times 0.85 is 30.6; the way is (1000 - 100) times
/// 1.25 = 1125 m, so 1125 / 30.6 + the call's 1.5 s. Find: five pings of
/// 2.5 s and 400 m at 5.5 m/s. Sample: 8 units at 550 per-mille is 4.4, four
/// units a 4 s cycle, so four cycles. Home is the same way back (1250 m, the
/// rider ended at the centre) and a screen to stow the lot. A session is
/// 45 s, a screen 20.
#[test]
fn a_tiny_path_is_worked_by_hand() {
    let registries = registries();
    let file = file(
        r#"[
            (what: Buy("veyra:vehicle.courier_bike")),
            (what: Ride(Nearest("strata:conductor"))),
            (what: Find),
            (what: Sample(15)),
            (what: Home),
            (what: Craft(schematic: "veyra:schematic.harness", by: "Frontier ranger")),
        ]"#,
    );
    let line = go(&registries, &file, &Variant::default());
    assert!(line.stopped().is_none(), "{:?}", line.stopped());
    let secs: Vec<f64> = line.rows.iter().map(|r| r.secs).collect();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
    assert!(near(secs[0], 20.0), "{secs:?}");
    assert!(near(secs[1], 1.5 + 1125.0 / 30.6), "{secs:?}");
    assert!(near(secs[2], 12.5 + 400.0 / 5.5), "{secs:?}");
    assert!(near(secs[3], 16.0), "{secs:?}");
    assert!(near(secs[4], 1.5 + 1250.0 / 30.6 + 20.0), "{secs:?}");
    assert!(near(secs[5], 45.0), "{secs:?}");
    // 2500 less the bike's 450.
    assert_eq!(line.rows[0].purse, 2050);
    // Conductance is 0.8 of Conductivity and 0.2 of Purity, both 800: 800
    // as the ceiling, which the points of a ranger with no trade do not
    // all reach.
    assert!(
        line.rows[5].made.contains("..800)"),
        "{}",
        line.rows[5].made
    );
    // Find 120; the first cycle 6 * 550 / 1000 = 3 and the first-sample 60,
    // three more of 3: 192. The harness is complexity 1: 40 and the first
    // 150. A first batch only sets the household's bests and beats none, so
    // the 50 a line beaten is not paid: 190.
    let last = line.rows.last().unwrap();
    assert!(
        last.xp.contains("Frontier: Survey 192, Fab 190"),
        "{}",
        last.xp
    );
}

#[test]
fn every_kind_of_step_runs_once() {
    let registries = registries();
    let file = file(
        r#"[
            (what: Take("Ilse Marrow")),
            (what: Buy("veyra:vehicle.courier_bike")),
            (what: Buy("veyra:station.field_harvester")),
            (what: Ride(Nearest("strata:conductor"))),
            (what: Find),
            (what: Sample(15)),
            (what: Plant),
            (what: Home),
            (what: Collect),
            (what: Home),
            (what: Take("Tovan Reyes")),
            (what: Craft(schematic: "veyra:schematic.cell_core", by: "Tovan Reyes")),
            (what: Sell(item: "veyra:component.cell_core", count: 1)),
            (what: Learn(id: "fabricator.batches.1", member: "Tovan Reyes")),
        ]"#,
    );
    let line = go(&registries, &file, &Variant::default());
    assert!(line.stopped().is_none(), "{}", table(&line));
    assert_eq!(line.rows.len(), 14);
    // Bike 450 and a machine 300 out of 2500.
    assert_eq!(line.rows[1].purse, 2050);
    assert_eq!(line.rows[2].purse, 1750);
    // A cell core of charge density 800 at the ceiling is 300 over the 500
    // minimum, a premium of 156 on the price of 120; the points of a first
    // session reach part of it, so the pay is over the base and not over that.
    assert!(
        line.rows[12].purse > 1750 && line.rows[12].purse <= 1750 + 156,
        "{}",
        line.rows[12].purse
    );
    // The machine made what the dial says in the time the path took, and
    // the harvest's Survey XP is paid on the units.
    let made = &line.rows[8].made;
    let units: u32 = made
        .split(" units after")
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{made}"));
    assert!(units > 0, "{made}");
    assert_ne!(line.rows[7].xp, line.rows[8].xp, "the harvest pays XP");
    assert!(
        line.rows[13].made.contains("learns"),
        "{}",
        line.rows[13].made
    );
}

/// A ride that asks for a line goes past a deposit that cannot give it.
#[test]
fn a_best_ride_skips_the_deposit_that_is_too_poor() {
    let registries = registries();
    let mut weak = deposit(&registries, "strata:red_conductor", 500.0, 4);
    for (_, figure) in &mut weak.figures {
        *figure = 300;
    }
    let ground = Fixed(vec![
        weak,
        deposit(&registries, "strata:red_conductor", 1000.0, 1),
    ]);
    let file = file(
        r#"[
            (what: Ride(Nearest("strata:conductor"))),
            (what: Ride(Best(class: "strata:conductor", schematic: "veyra:schematic.harness", line: "conductance", at_least: 700))),
        ]"#,
    );
    let line = run(&registries, &ground, &file, HOME, FOOT, &Variant::default());
    assert!(line.stopped().is_none(), "{}", table(&line));
    assert!(line.rows[0].made.contains("500 m"), "{}", line.rows[0].made);
    assert!(
        line.rows[1].made.contains("1000 m"),
        "{}",
        line.rows[1].made
    );
}

#[test]
fn a_refused_step_stops_the_clock_and_says_why() {
    let registries = registries();
    let file = file(
        r#"[
            (what: Buy("veyra:vehicle.courier_bike")),
            (what: Craft(schematic: "veyra:schematic.harness", by: "Frontier ranger")),
            (what: Buy("veyra:tool.survey")),
        ]"#,
    );
    let line = go(&registries, &file, &Variant::default());
    assert_eq!(line.rows.len(), 2, "the third step is never run");
    let stopped = line.stopped().unwrap();
    assert!(
        stopped.refused.as_ref().unwrap().contains("short of 15"),
        "{stopped:?}"
    );
    assert!((stopped.secs).abs() < 1e-9 && (stopped.clock_s - 20.0).abs() < 1e-9);
    // A step away from the bench, or by a member who is not out, is refused
    // as well.
    let file = self::file(
        r#"[(what: Ride(Nearest("strata:conductor"))), (what: Buy("veyra:vehicle.courier_bike"))]"#,
    );
    let line = go(&registries, &file, &Variant::default());
    assert!(
        line.stopped()
            .unwrap()
            .refused
            .as_ref()
            .unwrap()
            .contains("away from home"),
        "{}",
        table(&line)
    );
    let file =
        self::file(r#"[(what: Craft(schematic: "veyra:schematic.harness", by: "Tovan Reyes"))]"#);
    let line = go(&registries, &file, &Variant::default());
    assert!(
        line.stopped()
            .unwrap()
            .refused
            .as_ref()
            .unwrap()
            .contains("is not out"),
        "{}",
        table(&line)
    );
    // A schematic not taught is a row that says so too.
    let file = self::file(
        r#"[(what: Craft(schematic: "veyra:schematic.patrol_carbine", by: "Frontier ranger"))]"#,
    );
    let line = go(&registries, &file, &Variant::default());
    assert!(
        line.stopped()
            .unwrap()
            .refused
            .as_ref()
            .unwrap()
            .contains("weaponsmith")
    );
    // And a deposit that is not there.
    let file = self::file(r#"[(what: Ride(Nearest("strata:brine"))), (what: Home)]"#);
    let bare = run(
        &registries,
        &Fixed::default(),
        &file,
        HOME,
        FOOT,
        &Variant::default(),
    );
    assert_eq!(bare.rows.len(), 1);
    assert!(
        bare.stopped()
            .unwrap()
            .refused
            .as_ref()
            .unwrap()
            .contains("within")
    );
}

/// Routes 2 makes the bike a tenth faster off the road and nothing else: the
/// rides differ and no other row does.
#[test]
fn a_box_changes_the_rows_it_acts_on_and_only_those() {
    let registries = registries();
    let file = file(
        r#"[
            (what: Take("Ilse Marrow")),
            (what: Buy("veyra:vehicle.courier_bike")),
            (what: Ride(Nearest("strata:conductor"))),
            (what: Find),
            (what: Sample(15)),
            (what: Home),
        ]"#,
    );
    let plain = go(&registries, &file, &Variant::default());
    let routed = go(
        &registries,
        &file,
        &Variant {
            grant: vec!["surveyor.routes.2".into()],
            ..Variant::default()
        },
    );
    assert_eq!(plain.rows.len(), routed.rows.len());
    for (n, (a, b)) in plain.rows.iter().zip(&routed.rows).enumerate() {
        let rides = matches!(n, 2 | 5);
        assert_eq!(
            a.secs > b.secs,
            rides,
            "row {n}: {} against {}",
            a.secs,
            b.secs
        );
        assert!(rides || (a.secs - b.secs).abs() < 1e-9, "row {n}");
    }
}

#[test]
fn the_harvest_numbers_come_from_the_dials() {
    let registries = registries();
    let shipped = FirstHour::load(&assets(), &registries).unwrap();
    let numbers = harvest_numbers(&registries, &shipped, "Ilse Marrow").unwrap();
    let dials = registries.resources.harvest();
    let want = u64::from(dials.rate) * u64::from(shipped.sample_at_pm) / 1000;
    assert_eq!(u64::from(numbers.units_hour), want);
    assert!(
        (numbers.hopper_hours - f64::from(dials.hopper) / f64::from(u32::try_from(want).unwrap()))
            .abs()
            < 0.01
    );
}
