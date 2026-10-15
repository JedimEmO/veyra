//! Tests of the bar in the household (ticket 0010, step 5): kept with the
//! member, saved, carried by a swap, and pressed by `--grant` and `--press`.

use super::*;
use strata::sim::ability::Place;
use strata::sim::combat::{CombatIntent, CombatMotion};

const PRECISION: &str = "precision_shot";

/// A household of the first day with a second member on the bench, whose
/// active member has been given the precision shot.
fn granted(registries: &GameRegistries) -> Household {
    let mut household = Household::starting(registries, &Grants::default());
    assert!(
        join_all(&mut household).len() > 1,
        "the first day has recruits to join"
    );
    household
        .grant_at_start(&[PRECISION.to_owned()], registries)
        .expect("the precision shot is a row");
    household
}

/// Each member's bar is their own, and the file keeps it: the one granted
/// the precision shot has it at the pistol's first place after a save and a
/// load, and the other's is untouched.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn the_bar_is_saved_and_read_back_per_member() {
    let registries = shipped();
    let household = granted(&registries);
    assert_eq!(household.check(&registries), Ok(()));
    let first = &household.characters[0].bar;
    assert_eq!(first.get(Place::Weapon(Weapon::Pistol, 1)), Some(PRECISION));
    assert!(household.characters[1].bar.places_of(PRECISION).is_empty());

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    household.save(&path).unwrap();
    let Start {
        household: read, ..
    } = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert_eq!(read.characters[0].bar, household.characters[0].bar);
    assert_eq!(read.characters[1].bar, household.characters[1].bar);
    assert_eq!(
        read.bar().get(Place::Weapon(Weapon::Pistol, 1)),
        Some(PRECISION),
        "the player spawns with the active member's bar"
    );
}

/// A status borne ends at a member switch: the one who comes out is built
/// anew, and the one who left takes none away.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_switch_ends_every_status() {
    let registries = shipped();
    let mut household = granted(&registries);
    let (mut actor, mut sheet, mut name, mut tint, mut pack, mut hands) =
        household.stats(&registries);
    let mut bar = household.bar();
    for id in ["slowed", "shield_broken", "disarmed", "reflecting"] {
        actor.give_status(id, 30.);
    }
    assert!(actor.disarmed());
    let mut input = CombatIntent::default();
    swap_to(
        &mut household,
        1,
        &registries,
        None,
        None,
        super::super::bench::Out {
            position: WorldPos::new(0., 0., 0.),
            actor: &mut actor,
            sheet: &mut sheet,
            input: &mut input,
            name: &mut name,
            tint: &mut tint,
            pack: &mut pack,
            hands: &mut hands,
            bar: &mut bar,
        },
    )
    .unwrap();
    assert!(actor.statuses().is_empty());
    assert!(!actor.disarmed() && !actor.shield_locked() && actor.reflects().is_none());
}

/// A file from before the bar has none and reads as an empty one, which the
/// sim fills from what the member's boxes grant.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_file_from_before_the_bar_reads_with_an_empty_one() {
    let registries = shipped();
    let household = Household::starting(&registries, &Grants::default());
    let text = ron::ser::to_string(&household).unwrap();
    assert!(text.contains("bar:"), "the bar is written");
    let older = text.replacen("bar:(weapon:{},utility:[],known:[],granted:[])", "", 1);
    assert_ne!(older, text, "the empty bar was written as {text}");
    let (read, ..) = Household::read(&older, &registries, &Grants::default).unwrap();
    assert!(read.characters.iter().all(|c| c.bar == Bar::default()));
}

/// The sim's changes to the player's bar are copied to the household, and a
/// swap carries each member's own.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_change_is_copied_and_a_swap_carries_each_members_bar() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let mut household = granted(&registries);
    let mut world = World::new();
    let (mut actor, mut sheet, mut name, mut tint, mut pack, mut hands) =
        household.stats(&registries);
    let mut bar = household.bar();
    let player = world.spawn((LocalPlayer, bar.clone())).id();
    world.insert_resource(household.clone());
    // A request of a screen moves the ability to the pistol's third place.
    let mut moved = bar.clone();
    moved.clear(Place::Weapon(Weapon::Pistol, 1));
    moved.set(Place::Weapon(Weapon::Pistol, 2), PRECISION);
    *world.get_mut::<Bar>(player).unwrap() = moved.clone();
    world.run_system_once(track_bar).unwrap();
    assert_eq!(world.resource::<Household>().characters[0].bar, moved);

    household = world.resource::<Household>().clone();
    bar = moved.clone();
    let mut input = CombatIntent::default();
    swap_to(
        &mut household,
        1,
        &registries,
        None,
        None,
        super::super::bench::Out {
            position: WorldPos::new(0., 0., 0.),
            actor: &mut actor,
            sheet: &mut sheet,
            input: &mut input,
            name: &mut name,
            tint: &mut tint,
            pack: &mut pack,
            hands: &mut hands,
            bar: &mut bar,
        },
    )
    .unwrap();
    assert_eq!(
        household.characters[0].bar, moved,
        "the one who left keeps theirs"
    );
    assert_eq!(
        bar, household.characters[1].bar,
        "the one who came has theirs"
    );
    assert!(bar.places_of(PRECISION).is_empty());
}

/// `--grant precision_shot --press 1@5` in a run: the member knows the shot,
/// the pistol's first place is pressed on frame 5, and the sim's rule begins
/// it and a bolt leaves at the table's figures, which the combatant reports.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_granted_precision_shot_is_pressed_and_reported() {
    use bevy::diagnostic::FrameCount;
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    household
        .grant_at_start(&[PRECISION.to_owned()], &registries)
        .unwrap();
    assert!(
        household
            .grant_at_start(&["no_such_skill".to_owned()], &registries)
            .is_err(),
        "an unknown id is refused"
    );
    let cli = crate::Cli::parse_from([
        "game",
        "--grant",
        PRECISION,
        "--press",
        "1@5,3@9",
        "--household",
        "scratch/combat/bar-test.ron",
    ]);
    assert_eq!(cli.press, vec![(1, 5), (3, 9)]);
    assert_eq!(cli.grant, [PRECISION]);

    let mut world = World::new();
    world.insert_resource(cli);
    world.insert_resource(FrameCount(4));
    let (actor, ..) = household.stats(&registries);
    let mut actor = actor.carrying([Some(Weapon::Carbine), Some(Weapon::Pistol)], 1);
    actor.set_bar(&household.bar());
    let shots = actor.shots;
    let ammo = actor.ammo;
    let player = world
        .spawn((LocalPlayer, CombatIntent::default(), actor))
        .id();
    world.run_system_once(crate::press_for_shot).unwrap();
    assert_eq!(world.get::<CombatIntent>(player).unwrap().ability, None);
    world.insert_resource(FrameCount(5));
    world.run_system_once(crate::press_for_shot).unwrap();
    assert_eq!(world.get::<CombatIntent>(player).unwrap().ability, Some(1));

    let mut input = *world.get::<CombatIntent>(player).unwrap();
    let mut actor = world.get::<Combatant>(player).unwrap().clone();
    let mut motion = CombatMotion::default();
    let mut leaving = None;
    for _ in 0..90 {
        let (_, shot) = actor.step(
            &mut input,
            &strata::sim::PlayerIntent::default(),
            1. / 60.,
            &mut motion,
        );
        if shot {
            leaving = Some((actor.shot_factors(), actor.shot_cone()));
        }
    }
    assert_eq!(
        leaving,
        Some(((2.2, 1.5), 0.)),
        "one exact bolt leaves at the table's figures"
    );
    assert_eq!((actor.shots, actor.ammo), (shots + 1, ammo - 1));
    assert!(actor.cooldown_left(PRECISION) > 0.);
}

/// The F5 reset builds the actor anew from the character: it must come with
/// the bar, since nothing changes the sheet to copy it in again.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn an_actor_built_for_a_reset_carries_the_members_bar() {
    let registries = shipped();
    let household = granted(&registries);
    let actor = household
        .active()
        .actor(&registries.progression, &registries.items);
    assert_eq!(actor.bar(), &household.bar());
    assert!(!household.bar().places_of(PRECISION).is_empty());
}

/// The whole path in a headless app: `--grant`, `--in-hand` the pistol (the carbine is out, so the first slot is drawn) and
/// `--press 1@N` go through `household::init`, the spawn, the tick and the
/// scheduler, and the player's combatant shows the shot.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn a_granted_precision_shot_is_pressed_in_a_headless_app() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--no-hostiles",
        "--fresh",
        "--household",
        path.to_str().unwrap(),
        "--in-hand",
        "veyra:weapon.service_pistol,veyra:weapon.service_blade",
        "--grant",
        PRECISION,
        "--press",
        "1@100000",
    ]);
    cli.resolve_play_mode();
    crate::install_combat_tables(&cli).unwrap();
    let mut app = crate::build(cli, None).unwrap();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1. / 60.),
    ));
    app.finish();
    app.cleanup();
    let read = |app: &mut App| {
        let mut query = app
            .world_mut()
            .query_filtered::<&Combatant, With<LocalPlayer>>();
        query.single(app.world()).ok().map(|actor| {
            (
                actor.shots,
                actor.ammo,
                actor.stamina,
                actor.cooldown_left(PRECISION),
            )
        })
    };
    // The world loads on threads at wall speed; the sim then runs one tick a
    // frame. The press is set for ten frames after the player appears.
    let started = std::time::Instant::now();
    let before = loop {
        app.update();
        if let Some(seen) = read(&mut app) {
            break seen;
        }
        assert!(
            started.elapsed().as_secs() < 120,
            "the player never spawned"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    let now = app.world().resource::<bevy::diagnostic::FrameCount>().0;
    app.world_mut().resource_mut::<crate::Cli>().press = vec![(1, now + 10)];
    for _ in 0..240 {
        app.update();
    }
    let after = read(&mut app).unwrap();
    assert_eq!(after.0, before.0 + 1, "one bolt left: {before:?} {after:?}");
    assert_eq!(after.1, before.1 - 1, "from the magazine");
    assert!(after.3 > 0., "and the ability cools down");
}

/// Counts the strikes the tick sends, which a test reads where the physics
/// would.
#[derive(Resource, Default)]
struct StrikesSent(u32);

fn count_strikes(
    mut attacks: MessageReader<strata::sim::combat::AttackRequest>,
    mut sent: ResMut<StrikesSent>,
) {
    for attack in attacks.read() {
        if matches!(attack.kind, strata::sim::combat::AttackKind::Strike { .. }) {
            sent.0 += 1;
        }
    }
}

/// `--grant thrust --in-hand` the blade `--press 1@N`: the member is carried
/// by the movement and the strike it ends in is sent, through the real tick.
/// Only the request is counted, not that it landed, and no hostile is placed
/// ahead: the physics tests hold the landing.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn a_granted_thrust_carries_the_member_and_sends_its_strike() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--fresh",
        "--household",
        path.to_str().unwrap(),
        "--in-hand",
        "veyra:weapon.service_blade",
        "--grant",
        "thrust",
        "--press",
        "1@100000",
    ]);
    cli.resolve_play_mode();
    crate::install_combat_tables(&cli).unwrap();
    let mut app = crate::build(cli, None).unwrap();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1. / 60.),
    ));
    app.init_resource::<StrikesSent>();
    app.add_systems(FixedUpdate, count_strikes);
    app.finish();
    app.cleanup();
    let read = |app: &mut App| {
        let mut query = app
            .world_mut()
            .query_filtered::<(&Combatant, &WorldPosition), With<LocalPlayer>>();
        query
            .single(app.world())
            .ok()
            .map(|(actor, at)| (actor.cooldown_left("thrust"), at.0))
    };
    let started = std::time::Instant::now();
    let before = loop {
        app.update();
        if let Some(seen) = read(&mut app) {
            break seen;
        }
        assert!(
            started.elapsed().as_secs() < 120,
            "the player never spawned"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    // Let the spawn settle on the ground before the press: under a loaded
    // machine the ground streams in late, so wait for the member to stand
    // on it (grounded, and at rest for sixty frames) rather than for a
    // fixed count of frames.
    let grounded = |app: &mut App| {
        let mut query = app
            .world_mut()
            .query_filtered::<&strata::physics::CharacterController, With<LocalPlayer>>();
        query.single(app.world()).is_ok_and(|c| c.grounded)
    };
    let mut still = 0;
    let mut last = before.1;
    let began = std::time::Instant::now();
    while still < 60 {
        app.update();
        let at = read(&mut app).unwrap().1;
        still = if grounded(&mut app) && at.0.distance(last.0) < 1e-6 {
            still + 1
        } else {
            0
        };
        last = at;
        assert!(began.elapsed().as_secs() < 90, "the member never settled");
    }
    let settled = last;
    let strikes = app.world().resource::<StrikesSent>().0;
    let now = app.world().resource::<bevy::diagnostic::FrameCount>().0;
    app.world_mut().resource_mut::<crate::Cli>().press = vec![(1, now + 5)];
    for _ in 0..120 {
        app.update();
    }
    let after = read(&mut app).unwrap();
    assert!(before.0 == 0., "the ability starts ready");
    assert!(after.0 > 0., "the ability cools down: it was pressed");
    let moved = after.1.0.distance(settled.0);
    assert!(
        (5. ..=7.).contains(&moved),
        "carried about its six metres, moved {moved}"
    );
    assert_eq!(
        app.world().resource::<StrikesSent>().0,
        strikes + 1,
        "and the strike it ends in was sent"
    );
}

/// `character`'s bar once the sim has held it to what the member knows, the
/// way `follow_sheet` does it when the member is out and a swap does it when
/// they come out.
fn followed(character: &Character, registries: &GameRegistries) -> Bar {
    let tables = strata::sim::combat_tables::current();
    let mut bar = character.bar.clone();
    let known = strata::sim::ability::known_to(&registries.progression, &character.sheet, &bar);
    bar.follow(&tables.abilities, &known);
    bar
}

/// The ids on a class's places, in order, the empty ones left out.
fn placed(bar: &Bar, weapon: Weapon) -> Vec<String> {
    (0..3)
        .filter_map(|at| bar.get(Place::Weapon(weapon, at)))
        .map(str::to_owned)
        .collect()
}

/// What each member of the first day has on each bar (ticket 0010, step 7):
/// a known ability goes in the first empty place of each class it works with,
/// in the order of abilities.ron. The same figures are written in the ticket.
#[test]
fn the_first_days_members_have_these_bars() {
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    let mut everyone = household.characters.clone();
    everyone.append(&mut household.recruits);
    let by_name = |name: &str| everyone.iter().find(|c| c.name == name).expect(name);
    let list = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();

    let arrival = followed(by_name("Frontier ranger"), &registries);
    assert_eq!(placed(&arrival, Weapon::Blade), list(&["heavy_cut"]));
    assert_eq!(placed(&arrival, Weapon::Greatblade), list(&["heavy_cut"]));
    assert_eq!(placed(&arrival, Weapon::Glaive), list(&["heavy_cut"]));
    assert_eq!(
        placed(&arrival, Weapon::Carbine),
        list(&["butt_strike", "aimed_burst"])
    );
    assert_eq!(placed(&arrival, Weapon::Pistol), list(&["butt_strike"]));
    assert_eq!(placed(&arrival, Weapon::Rifle), list(&["butt_strike"]));

    let tovan = followed(by_name("Tovan Reyes"), &registries);
    assert_eq!(
        placed(&tovan, Weapon::Blade),
        list(&["heavy_cut", "disarm", "thrust"])
    );
    assert_eq!(placed(&tovan, Weapon::Greatblade), list(&["heavy_cut"]));
    assert_eq!(
        placed(&tovan, Weapon::Glaive),
        list(&["heavy_cut", "thrust"])
    );
    assert_eq!(
        placed(&tovan, Weapon::Carbine),
        list(&["butt_strike", "aimed_burst", "suppressing_fire"])
    );
    assert_eq!(
        placed(&tovan, Weapon::Pistol),
        list(&["butt_strike", "disarm"])
    );
    assert_eq!(placed(&tovan, Weapon::Rifle), list(&["butt_strike"]));

    let ilse = followed(by_name("Ilse Marrow"), &registries);
    assert_eq!(
        placed(&ilse, Weapon::Pistol),
        list(&["butt_strike", "precision_shot", "disarm"])
    );
    assert_eq!(placed(&ilse, Weapon::Carbine), list(&["butt_strike"]));
    assert_eq!(placed(&ilse, Weapon::Rifle), list(&["butt_strike"]));
    assert_eq!(placed(&ilse, Weapon::Blade), list(&["disarm"]));

    let anneke = followed(by_name("Dr. Anneke Faal"), &registries);
    assert!(anneke.known.is_empty(), "the medic knows nothing yet");
    assert!(anneke.utility.iter().all(Option::is_none));
    for weapon in Weapon::ALL {
        assert!(placed(&anneke, weapon).is_empty());
    }
}

/// A household written before step 7 (a member who learned the Cuts 1 to 3,
/// Breaking 1 and Pistol 1, with no bar in the file) loads as it was, and the
/// bar the sim then holds to what they know has the heavy cut where it was
/// got by Cuts 3 and the rest placed where there was room. Nothing refunded.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn an_older_member_loads_unchanged_and_their_bar_is_filled_from_what_they_know() {
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    let boxes = [
        "blade.novice",
        "blade.cuts.1",
        "blade.cuts.2",
        "blade.cuts.3",
        "blade.breaking.1",
        "marksman.novice",
        "marksman.pistol.1",
    ];
    let character = &mut household.characters[0];
    character.sheet.learned = boxes.iter().map(|id| (*id).to_owned()).collect();
    character.sheet.xp.insert(XpKind::Blade, 900);
    character.sheet.xp.insert(XpKind::Ranged, 150);
    character.bar = Bar::default();
    let spent = character.sheet.points_spent(&registries.progression);
    let text = ron::ser::to_string(&household).unwrap();
    // The file of a household from before the bar has none; the empty bar is
    // dropped from the text as a file from before it would not have it.
    let older = text.replacen("bar:(weapon:{},utility:[],known:[],granted:[])", "", 1);
    assert_ne!(older, text);

    let (read, upgraded, mended) = Household::read(&older, &registries, &Grants::default).unwrap();
    assert!(upgraded.is_none() && mended.is_empty(), "read as it was");
    let sheet = &read.characters[0].sheet;
    assert_eq!(sheet.learned, household.characters[0].sheet.learned);
    assert_eq!(sheet.xp, household.characters[0].sheet.xp);
    assert_eq!(sheet.points_spent(&registries.progression), spent);
    assert_eq!(read.characters[0].bar, Bar::default());

    let tables = strata::sim::combat_tables::current();
    let fitted = strata::sim::progression::apply(&tables, &registries.progression, sheet);
    assert!(
        fitted
            .unlocked
            .contains(&strata::sim::combat::Action::Heavy),
        "they still cut heavily"
    );
    let bar = followed(&read.characters[0], &registries);
    let heavy = ["heavy_cut".to_owned()];
    for weapon in [Weapon::Blade, Weapon::Greatblade] {
        assert!(
            placed(&bar, weapon).starts_with(&heavy),
            "{weapon:?} cuts heavily: {:?}",
            placed(&bar, weapon)
        );
    }
    // The blade has three places, all taken before the reflect shot, which
    // is known and waits in the loadout tab for a place to be made.
    assert!(bar.known.contains("reflect_shot"));
    assert_eq!(placed(&bar, Weapon::Blade).len(), 3);
    for (ability, weapon) in [
        ("thrust", Weapon::Blade),
        ("disarm", Weapon::Blade),
        ("precision_shot", Weapon::Pistol),
    ] {
        assert!(
            placed(&bar, weapon).contains(&ability.to_owned()),
            "{ability} is on the {weapon:?}: {:?}",
            placed(&bar, weapon)
        );
    }
}
