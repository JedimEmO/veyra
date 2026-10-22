//! A bout is a function of its seed and the installed tables: the same
//! seed on another thread, in another app, gives the same row, bit for
//! bit, or the harness cannot be trusted to compare two table values.

use balance::report::bout_row;
use balance::{Cell, Policy, Profile, Side, Winner, bout, preset};
use veyra_rules::combat::Weapon;

#[test]
fn the_same_seed_and_tables_give_the_same_row() {
    let matchup = preset("carbine-vs-rusher").expect("the preset");
    let cell = Cell { values: Vec::new() };
    let rows: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let matchup = &matchup;
                let cell = &cell;
                scope.spawn(move || {
                    let result = bout::run(matchup, 7, 60.);
                    bout_row(cell, &matchup.name, 7, &result)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a bout thread"))
            .collect()
    });
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    let again = bout::run(&matchup, 7, 60.);
    assert_eq!(bout_row(&cell, &matchup.name, 7, &again), rows[0]);
    // The row is not trivial: a different seed reacts at a different
    // moment, and the matchup's bouts of a cell spread because of it.
    let other = bout::run(&matchup, 8, 60.);
    assert_eq!(other.winner, Winner::A);
    assert!(
        (other.time - again.time).abs() > 1e-6,
        "seed 7 {} s, seed 8 {} s",
        again.time,
        other.time
    );
}

/// A cutting hand draws from the seed (its arrival, its pauses, the
/// centimetre it stops at, the lag of its return cut), so two seeds of a
/// blade against the rusher, whose brain draws nothing, differ, and one
/// seed twice is the same to the bit.
#[test]
fn a_cutting_hand_differs_with_its_seed_and_repeats_with_one() {
    let matchup = preset("blade-vs-rusher").expect("the preset");
    let cell = Cell { values: Vec::new() };
    let first = bout::run(&matchup, 1, 60.);
    let again = bout::run(&matchup, 1, 60.);
    assert_eq!(first, again);
    assert_eq!(
        bout_row(&cell, &matchup.name, 1, &first),
        bout_row(&cell, &matchup.name, 1, &again)
    );
    let other = bout::run(&matchup, 2, 60.);
    assert!(
        (other.time - first.time).abs() > 1e-6
            || (other.sides[0].taken - first.sides[0].taken).abs() > 1e-6,
        "seed 1 {first:?}, seed 2 {other:?}"
    );
}

/// A bout that only shifted by the arrival wait would show at most sixteen
/// end times; a bare blade against the rusher shows more than a dozen in
/// forty seeds, and a blade that presses abilities against the duelist,
/// whose clock does not swallow the hand's, shows more than sixteen.
#[test]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "milliseconds of a bout under a minute"
)]
fn a_cutting_hand_is_more_than_a_shifted_bout() {
    for (name, least) in [("blade-vs-rusher", 12), ("disarm-blade-vs-duelist", 16)] {
        let matchup = preset(name).expect("the preset");
        let mut ends: Vec<(u32, u32)> = (1..=40)
            .map(|seed| {
                let result = bout::run(&matchup, seed, 60.);
                (
                    (result.time * 1000.).round() as u32,
                    (result.sides[0].taken * 10.).round() as u32,
                )
            })
            .collect();
        ends.sort_unstable();
        ends.dedup();
        assert!(
            ends.len() > least,
            "{name}: {} distinct bouts in 40",
            ends.len()
        );
    }
}

/// A side that presses abilities has one clock for its cuts and its
/// presses, and the seed reaches it: over twenty seeds a blade that opens
/// with a thrust against the rusher ends at more than two moments.
#[test]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "milliseconds of a bout under a minute"
)]
fn a_hand_that_presses_abilities_still_has_a_seed() {
    let matchup = preset("worth-blade-vs-rusher").expect("the preset");
    let mut times: Vec<u32> = (1..=20)
        .map(|seed| (bout::run(&matchup, seed, 60.).time * 1000.).round() as u32)
        .collect();
    times.sort_unstable();
    times.dedup();
    assert!(times.len() >= 3, "{} distinct ends in 20", times.len());
}

/// A fitted side carries state of its own (its effective rows behind an
/// `Arc`, a magazine rounded by `WeaponSpec::modify`, the graph read from
/// the process-wide slot when the presets are built), none of which may
/// make two runs of one seed differ.
#[test]
fn a_boxed_matchup_is_as_deterministic_as_a_bare_one() {
    let matchup = preset("blade-master-vs-novice").expect("the preset");
    assert!(
        !matchup.a.boxes.is_empty() && !matchup.b.boxes.is_empty(),
        "both sides are fitted"
    );
    let cell = Cell { values: Vec::new() };
    let rows: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let matchup = &matchup;
                let cell = &cell;
                scope.spawn(move || {
                    let result = bout::run(matchup, 11, 60.);
                    bout_row(cell, &matchup.name, 11, &result)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a bout thread"))
            .collect()
    });
    assert_eq!(rows[0], rows[1]);
    let first = bout::run(&matchup, 11, 60.);
    let again = bout::run(&matchup, 11, 60.);
    assert_eq!(first, again);
    assert_eq!(bout_row(&cell, &matchup.name, 11, &again), rows[0]);
    assert_ne!(first.winner, Winner::Draw, "{first:?}");
}

#[test]
fn a_blade_duel_lands_cuts_and_both_sides_books_agree() {
    let blade = Side {
        policy: Policy::CloseAndCut {
            chain: true,
            heavy_every: 3,
            cooldown: 0.,
            reach: balance::BLADE_RANGE,
            wait_stamina: false,
            sprint: false,
        },
        weapon: Weapon::Blade,
        profile: Profile::Player,
        count: 1,
        boxes: Vec::new(),
        fitted: Vec::new(),
        condition: None,
        lines: None,
        abilities: Vec::new(),
        with: Vec::new(),
        tier: 0,
    };
    let matchup = bout::Matchup {
        name: "blade-vs-blade".into(),
        a: blade.clone(),
        b: blade,
        distance: 8.,
    };
    let result = bout::run(&matchup, 3, 60.);
    assert_ne!(result.winner, Winner::Draw, "{result:?}");
    let landed = result.sides[0].cuts_landed + result.sides[1].cuts_landed;
    assert!(landed >= 3, "{result:?}");
    assert!(
        (result.sides[0].dealt - result.sides[1].taken).abs() < 1e-3
            && (result.sides[1].dealt - result.sides[0].taken).abs() < 1e-3,
        "{result:?}"
    );
}

/// The carbine's own-line presets and the sight's trio draw their spread
/// from a counter, so a bout with a cone in it is as repeatable as one
/// without.
#[test]
fn the_weapon_line_and_sight_presets_exist_and_repeat() {
    for name in [
        "bought-carbine-vs-dummy",
        "made-carbine-vs-dummy",
        "aimed-carbine-vs-dummy-27m",
        "sighted-carbine-vs-dummy-27m",
        "fine-sighted-carbine-vs-dummy-27m",
        "bought-blade-vs-dummy",
        "made-blade-vs-dummy",
        "hooked-blade-vs-dummy",
    ] {
        let matchup = preset(name).expect("the preset");
        assert_eq!(bout::run(&matchup, 3, 30.), bout::run(&matchup, 3, 30.));
    }
}

/// The marksman, the duelist and the bulwark are the game's brains on their sheets: two
/// runs of one seed, on two threads, give one row.
#[test]
fn the_marksman_the_duelist_and_the_bulwark_are_deterministic() {
    for name in [
        "rifle-vs-marksman-60m",
        "blade-guard-vs-duelist",
        // The shield bearer and the hand that waits for its shield to come down.
        "worth-blade-vs-bulwark",
        "punish-carbine-vs-bulwark",
        "blast-carbine-vs-bulwark",
    ] {
        let matchup = preset(name).expect("the preset");
        let cell = Cell { values: Vec::new() };
        let rows: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let (matchup, cell) = (&matchup, &cell);
                    scope.spawn(move || {
                        let result = bout::run(matchup, 5, 30.);
                        bout_row(cell, &matchup.name, 5, &result)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("a bout thread"))
                .collect()
        });
        assert_eq!(rows[0], rows[1], "{name}");
    }
}

/// The stop-and-shoot hand keeps a walk clock and a count of its own shots,
/// and the sentry and rusher sides are fitted from their sheets now: two
/// runs of one seed read the same row all the same.
#[test]
fn a_stop_and_shoot_hand_against_a_fitted_opponent_repeats() {
    for name in ["worth-rifle-vs-marksman", "worth-carbine-vs-sentry"] {
        let matchup = preset(name).expect("the preset");
        let cell = Cell { values: Vec::new() };
        let first = bout::run(&matchup, 5, 30.);
        let again = bout::run(&matchup, 5, 30.);
        assert_eq!(first, again, "{name}");
        assert_eq!(
            bout_row(&cell, &matchup.name, 5, &first),
            bout_row(&cell, &matchup.name, 5, &again)
        );
        assert_ne!(first.winner, Winner::Draw, "{name}: {first:?}");
    }
}
