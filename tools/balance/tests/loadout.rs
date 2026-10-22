//! A fitted module is in the fight: the patrol carbine's extended magazine
//! fires five more rounds before its first reload and pays for them with a
//! longer one, and a harness side shows both, on the real sim and physics.
//!
//! One test, because the tables it installs are the process's.

use balance::{Matchup, Policy, Profile, Side, bout, loadout};
use veyra_rules::combat::Weapon;
use veyra_rules::combat_tables::{self, CombatTables};

/// A carbine held at ten metres on a target that stands and does not
/// fall.
fn range(fitted: &[&str]) -> Matchup {
    Matchup {
        name: "carbine-on-the-range".to_owned(),
        a: Side {
            policy: Policy::HoldAndFire {
                range: 12.,
                cadence: 0.,
                aim: true,
            },
            weapon: Weapon::Carbine,
            profile: Profile::Player,
            count: 1,
            boxes: Vec::new(),
            fitted: fitted
                .iter()
                .map(|spec| loadout::parse(spec).expect("a shipped module"))
                .collect(),
            condition: None,
            // A bought carbine, which reads its own 1.10 of the interval,
            // so the bare side and the fitted ones fire at one rate.
            lines: Some(std::collections::BTreeMap::new()),
            abilities: Vec::new(),
            with: Vec::new(),
            tier: 0,
        },
        b: Side {
            policy: Policy::Stand,
            weapon: Weapon::Carbine,
            profile: Profile::Post,
            count: 1,
            boxes: Vec::new(),
            fitted: Vec::new(),
            condition: None,
            lines: None,
            abilities: Vec::new(),
            with: Vec::new(),
            tier: 0,
        },
        distance: 10.,
    }
}

#[test]
fn a_fitted_feed_fires_five_more_before_the_first_reload_and_pays_them_back() {
    // A target that outlasts the bout, so the trigger is held throughout
    // and the count is the magazine's and the reload's alone.
    let mut tables = CombatTables::shipped();
    // The post is the player's pools at the training target's health; a
    // fitted opponent would be held to its sheet's cap, so the target is
    // the unfitted post.
    tables.opponents.passive_health = 100_000.;
    // Only the magazine and the reload are under test, so the carbine
    // fires as it did before it had burst: a round every interval while
    // held. The burst and the hold have their own tests in the sim.
    let carbine = tables
        .weapons
        .weapons
        .get_mut(&Weapon::Carbine)
        .expect("the carbine's row");
    carbine.mode = veyra_rules::combat::FireMode::Auto;
    carbine.hold = None;
    carbine.interval = 0.24;
    combat_tables::install(tables);
    let bare = range(&[]);
    let fed = range(&["veyra:module.patrol_extended_feed"]);
    let made = range(&["veyra:module.patrol_extended_feed@capacity=610,reload_time=433"]);
    let shots = |matchup: &Matchup, cap: f32| bout::run(matchup, 1, cap).sides[0].shots;

    // At 7.5 s the bare carbine has emptied its 24 and is reloading; the
    // fitted one is still firing from a magazine of 29. Every side here is
    // a bought carbine, which fires at 0.264 s.
    assert_eq!(shots(&bare, 7.5), 24);
    assert_eq!(shots(&fed, 7.5), 28);
    // By 10 s the bare one is firing again and the fitted one is past its
    // longer reload: the five rounds are paid back, to within two.
    let (plain, extended) = (shots(&bare, 10.), shots(&fed, 10.));
    assert_eq!((plain, extended), (33, 33));
    assert!(plain.abs_diff(extended) <= 2);
    // The magazine is 29 and no more: the 29th round leaves by 7.8 s and
    // the 30th not before the reload of 1.485 s is done, where the bare
    // carbine's 1.35 has its 25th away by 7.8, 1.4 s after its 24th.
    assert_eq!([7.8, 9.0].map(|cap| shots(&fed, cap)), [29, 29]);
    assert!(shots(&fed, 9.5) >= 30);
    assert_eq!([7.6, 7.8].map(|cap| shots(&bare, cap)), [24, 25]);
    // Tovan Reyes's batch holds 29 as well and reloads a hair slower.
    assert_eq!(shots(&made, 7.5), 28);
    assert_eq!(shots(&made, 8.1), 29);

    // Nothing but the magazine and the reload moved: both land every
    // round on a standing target, at the bare carbine's damage.
    let (plain, extended) = (bout::run(&bare, 1, 7.5), bout::run(&fed, 1, 7.5));
    let per_hit = |side: &balance::SideStats| {
        side.dealt / f32::from(u16::try_from(side.hits.max(1)).expect("a few dozen hits"))
    };
    assert!(plain.sides[0].hits > 20 && extended.sides[0].hits > plain.sides[0].hits);
    assert!((per_hit(&plain.sides[0]) - per_hit(&extended.sides[0])).abs() < 1e-3);

    // A carbine worn to nothing fires slower over the same bout, a tired
    // one exactly as the bare one does.
    let at = |condition: u16| {
        let mut matchup = range(&[]);
        matchup.a.condition = Some(condition);
        matchup
    };
    let worn = shots(&at(0), 6.5);
    assert!(worn < shots(&bare, 6.5), "{worn}");
    assert_eq!(shots(&at(250), 6.5), shots(&bare, 6.5));
    combat_tables::install(CombatTables::shipped());
}

#[test]
fn a_carbine_worn_to_nothing_reads_the_worse_row_and_a_tired_one_the_unworn_row() {
    use veyra_rules::combat::Combatant;
    let row = |condition: Option<u16>| {
        let mut actor = Combatant::default();
        let side = Side {
            condition,
            ..range(&[]).a
        };
        if condition.is_some() {
            actor.equip(&loadout::of(&side).expect("a bought carbine at that condition"));
        }
        let spec = actor.weapon_spec();
        (spec.interval, spec.reload)
    };
    // No record reads the table; a bought carbine its own 1.10 of the
    // interval, which the wear then moves.
    assert!((row(None).0 - Weapon::Carbine.spec().interval).abs() < 1e-9);
    let (interval, reload) = row(Some(1000));
    assert!((interval - row(None).0 * 1.1).abs() < 1e-6, "{interval}");
    assert_eq!(row(Some(250)), (interval, reload));
    // 1.25 times the table's, exactly: the wear.ron `worst` for both.
    assert_eq!(row(Some(0)), (interval * 1.25, reload * 1.25));
    assert!(row(Some(125)).0 > interval && row(Some(125)).0 < interval * 1.25);
}

/// The three blade presets read the figures of a bought blade, a made one
/// and a bought one with the hooked edge, through the loadout the game
/// builds: the edge line is the cut's damage, the part is the cut's posture
/// and the tail after its contact.
#[test]
fn the_blade_presets_read_a_made_edge_and_a_fitted_part() {
    use veyra_rules::combat::{Action, Combatant};
    let read = |name: &str| {
        let matchup = balance::preset(name).expect("the preset");
        let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        actor.equip(&loadout::of(&matchup.a).expect("the blade's loadout"));
        (actor.spec(Action::Light), actor.spec(Action::Heavy))
    };
    let ((bought, bought_heavy), (made, _), (hooked, hooked_heavy)) = (
        read("bought-blade-vs-dummy"),
        read("made-blade-vs-dummy"),
        read("hooked-blade-vs-dummy"),
    );
    // 0.80 to 1.10 over the line: 0.95 at 500 and 1.07 at 900.
    assert!(
        (bought.damage - 28. * 0.95).abs() < 1e-4,
        "{}",
        bought.damage
    );
    assert!((made.damage - 28. * 1.07).abs() < 1e-4, "{}", made.damage);
    // The hooked edge changes the posture and the tail and not the damage.
    assert_eq!(hooked.damage.to_bits(), bought.damage.to_bits());
    assert!((hooked_heavy.posture - bought_heavy.posture * 1.08).abs() < 1e-3);
    assert_eq!(hooked.contact_end.to_bits(), bought.contact_end.to_bits());
    let tail = |spec: &veyra_rules::combat::ActionSpec| spec.duration - spec.contact_end;
    assert!((tail(&hooked) - tail(&bought) * 1.06).abs() < 1e-5);
}
