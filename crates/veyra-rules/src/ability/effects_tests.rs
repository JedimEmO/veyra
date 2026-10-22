//! Tests of the second half of ticket 0010, step 5: the four statuses that
//! land on someone, what a hit carries, the strike that reads its weapon,
//! the movement, and the eleven abilities built on them. What the physics
//! does with a bolt's landing is tested in `strata-physics`.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
#![allow(
    clippy::float_cmp,
    reason = "a table figure and a factor on it are compared to the bit"
)]

use super::test_support::ABILITIES;
use super::test_support::DT;
use super::test_support::gun_place;
use super::test_support::pistol;
use super::test_support::rig;
use super::test_support::sheet_of;
use super::test_support::table;
use super::test_support::tick;
use super::*;
use crate::combat::Action;
use crate::combat::Blow;
use crate::combat::BlowKind;
use crate::combat::CombatIntent;
use crate::combat::CombatMotion;
use crate::combat::Combatant;
use crate::combat::GuardOutcome;
use crate::combat::Refusal;
use crate::combat::StrikeBlow;
use crate::combat::Weapon;
use crate::combat_loadout::Factor;
use crate::combat_loadout::Loadout;
use crate::combat_tables;
use bevy_math::Vec2;
use bevy_math::Vec3;
use strata_registry::item::Property;
use strata_registry::progression::Effect;
use strata_registry::progression::Op;
use strata_registry::progression::SkillGraph;
use strata_types::PlayerIntent;

fn melee(weapon: Weapon) -> Combatant {
    Combatant::default().carrying([Some(weapon), None], 0)
}

fn reach_m(weapon: Weapon) -> f32 {
    combat_tables::with(|tables| tables.melee.reach_m(weapon))
}

/// What one press of place 0 let leave in `ticks`: each bolt's age, cone,
/// factors and effects, and each strike's age and figures.
struct Seen {
    bolts: Vec<(f32, f32, (f32, f32), ShotEffects)>,
    strikes: Vec<(f32, StrikeBlow)>,
}

fn press(actor: &mut Combatant, index: usize, ticks: usize) -> Seen {
    let mut input = CombatIntent {
        ability: Some(index),
        ..CombatIntent::default()
    };
    let mut seen = Seen {
        bolts: Vec::new(),
        strikes: Vec::new(),
    };
    for _ in 0..ticks {
        if tick(actor, &mut input) {
            seen.bolts.push((
                actor.age,
                actor.shot_cone(),
                actor.shot_factors(),
                actor.shot_effects().clone(),
            ));
        }
        if let Some(blow) = actor.take_strike() {
            seen.strikes.push((actor.age, blow));
        }
    }
    seen
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "every field it fills is an Option"
)]
fn on_hit(status: &str, seconds: f32) -> Option<OnHit> {
    Some(OnHit {
        status: status.to_owned(),
        seconds,
    })
}

fn rifle() -> Combatant {
    melee(Weapon::Rifle)
}

fn light_of(actor: &Combatant) -> (f32, f32) {
    let light = actor.spec(Action::Light);
    (light.damage, light.posture)
}

#[test]
fn the_new_checks_refuse_what_cannot_be_played() {
    let refused = |from: &str, to: &str| {
        let text = ABILITIES.replace(from, to);
        assert_ne!(text, ABILITIES, "{from}");
        let table: AbilityTable = ron::from_str(&text).expect("still RON");
        table.check().expect_err(from)
    };
    let hit = "status: \"disarmed\", seconds: 3.0";
    assert!(refused(hit, "status: \"dazed\", seconds: 3.0").contains("no row"));
    assert!(refused(hit, "status: \"\", seconds: 3.0").contains("no row"));
    assert!(refused(hit, "status: \"disarmed\", seconds: 0.0").contains("seconds"));
    assert!(
        refused(
            "on_hit: Some((status: \"slowed\"",
            "on_hit: Some((status: \"dazed\""
        )
        .contains("no row")
    );
    assert!(refused("jump: Some((radius: 8.0", "jump: Some((radius: 0.0").contains("jump"));
    assert!(refused("blast: Some((radius: 4.0", "blast: Some((radius: -1.0").contains("blast"));
    assert!(refused("Move(distance: 6.0,", "Move(distance: 0.0,").contains("move"));
    assert!(refused("seconds: 0.28", "seconds: 0.9").contains("pass the length"));
    assert!(
        refused(
            "Strike(reach: 1.0, arc: 110.0",
            "Strike(reach: 3.5, arc: 110.0"
        )
        .contains("factor of three")
    );
    assert!(refused("pace: Some(0.6)", "pace: Some(1.5)").contains("share"));
}

#[test]
fn the_eleven_rows_are_what_the_design_says() {
    let table = table();
    for id in [
        "ricochet",
        "disarm",
        "suppressing_fire",
        "explosive_shot",
        "charged_shot",
        "armour_break",
        "thrust",
        "reflect_shot",
        "sweeping_cut",
        "vault",
        "combat_slide",
    ] {
        assert!(table.row(id).is_some(), "{id}");
    }
    let slide = table.row("combat_slide").unwrap();
    assert!(slide.armed && slide.slot == SlotKind::Utility && slide.works_with.is_empty());
    assert!(
        table
            .abilities
            .iter()
            .filter(|row| row.id != "combat_slide")
            .all(|row| !row.armed),
        "only the slide keeps the weapon up"
    );
}

#[test]
fn ricochet_is_one_aimed_bolt_that_jumps() {
    let (mut actor, _) = rig(pistol(), &[(gun_place(Weapon::Pistol, 0), "ricochet")]);
    let (ammo, aimed) = (actor.ammo, actor.weapon_spec().aim_spread);
    let seen = press(&mut actor, 0, 60);
    assert_eq!(seen.bolts.len(), 1);
    let (age, cone, factors, effects) = &seen.bolts[0];
    assert!((age - 0.15).abs() < 0.02, "{age}");
    assert_eq!((*cone, *factors), (aimed, (1., 1.)));
    assert_eq!(
        effects,
        &ShotEffects {
            jump: Some(Jump {
                radius: 8.,
                share: 0.8
            }),
            ..ShotEffects::default()
        }
    );
    assert_eq!(actor.ammo, ammo - 1);
    assert_eq!(actor.bloom(), 0.);
    // The next plain round carries nothing.
    let mut input = CombatIntent {
        fire: true,
        ..CombatIntent::default()
    };
    for _ in 0..120 {
        if tick(&mut actor, &mut input) {
            assert_eq!(*actor.shot_effects(), ShotEffects::default());
            return;
        }
    }
    panic!("a plain round followed");
}

#[test]
fn disarm_is_one_bolt_with_a_gun_and_a_scaled_strike_with_a_blade() {
    let (mut gun, _) = rig(pistol(), &[(gun_place(Weapon::Pistol, 0), "disarm")]);
    let (ammo, aimed) = (gun.ammo, gun.weapon_spec().aim_spread);
    let seen = press(&mut gun, 0, 60);
    assert_eq!(seen.bolts.len(), 1);
    assert!(seen.strikes.is_empty());
    let (age, cone, factors, effects) = &seen.bolts[0];
    assert!((age - 0.10).abs() < 0.02, "{age}");
    assert_eq!((*cone, *factors), (aimed, (0.6, 0.5)));
    assert_eq!(effects.on_hit, on_hit("disarmed", 3.));
    assert!(!effects.breach && effects.jump.is_none() && effects.blast.is_none());
    assert_eq!(gun.ammo, ammo - 1, "a round from the magazine");
    assert_eq!(gun.bloom(), 0.);

    let (mut blade, _) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "disarm")],
    );
    let (damage, posture) = light_of(&blade);
    let seen = press(&mut blade, 0, 60);
    assert!(seen.bolts.is_empty(), "no round, no bolt");
    assert_eq!(seen.strikes.len(), 1);
    let (age, strike) = &seen.strikes[0];
    assert!((age - 0.10).abs() < 0.02, "{age}");
    assert_eq!(
        *strike,
        StrikeBlow {
            reach: reach_m(Weapon::Blade),
            arc: 40.,
            damage: 0.6 * damage,
            posture: 0.5 * posture,
            on_hit: on_hit("disarmed", 3.),
        }
    );
}

#[test]
fn a_hit_with_an_empty_gun_is_refused_for_rounds() {
    let (mut gun, mut input) = rig(pistol(), &[(gun_place(Weapon::Pistol, 0), "disarm")]);
    gun.ammo = 0;
    input.ability = Some(0);
    tick(&mut gun, &mut input);
    assert_eq!(
        gun.refused,
        Some(Refusal::Ability(AbilityRefusal::NoRounds))
    );
}

#[test]
fn suppressing_fire_is_ten_hip_rounds_that_slow() {
    let (mut actor, _) = rig(
        Combatant::default(),
        &[(gun_place(Weapon::Carbine, 0), "suppressing_fire")],
    );
    let (ammo, hip) = (actor.ammo, actor.weapon_spec().spread);
    let seen = press(&mut actor, 0, 120);
    assert_eq!(seen.bolts.len(), 10);
    for (_, cone, factors, effects) in &seen.bolts {
        assert_eq!((*cone, *factors), (hip, (0.6, 0.8)));
        assert_eq!(effects.on_hit, on_hit("slowed", 3.));
    }
    for pair in seen.bolts.windows(2) {
        assert!((pair[1].0 - pair[0].0 - 0.07).abs() < 0.02);
    }
    assert_eq!(actor.ammo, ammo - 10);
    assert_eq!(actor.bloom(), 0., "and no bloom");
    // The hip cone is wider than the aimed one it is not.
    assert!(hip > actor.weapon_spec().aim_spread);
}

#[test]
fn explosive_shot_carries_a_blast_and_charged_shot_a_breach() {
    let (mut carbine, _) = rig(
        Combatant::default(),
        &[(gun_place(Weapon::Carbine, 0), "explosive_shot")],
    );
    let aimed = carbine.weapon_spec().aim_spread;
    let seen = press(&mut carbine, 0, 90);
    assert_eq!(seen.bolts.len(), 1);
    let (age, cone, factors, effects) = &seen.bolts[0];
    assert!((age - 0.20).abs() < 0.02, "{age}");
    assert_eq!((*cone, *factors), (aimed, (1.6, 1.)));
    assert_eq!(
        effects.blast,
        Some(Blast {
            radius: 4.,
            damage: 48.,
            posture: 90.
        })
    );

    let (mut rifle, _) = rig(rifle(), &[(gun_place(Weapon::Rifle, 0), "charged_shot")]);
    let ammo = rifle.ammo;
    let seen = press(&mut rifle, 0, 120);
    assert_eq!(seen.bolts.len(), 1);
    let (age, cone, factors, effects) = &seen.bolts[0];
    assert!((age - 0.90).abs() < 0.02, "the charge is the windup: {age}");
    assert_eq!((*cone, *factors), (0., (1.1, 1.5)));
    assert!(effects.breach && effects.on_hit.is_none());
    assert_eq!(rifle.ammo, ammo - 1);
}

#[test]
fn armour_break_is_a_hit_that_shatters_the_shield_with_a_rifle_or_a_greatblade() {
    let (mut rifle, _) = rig(rifle(), &[(gun_place(Weapon::Rifle, 0), "armour_break")]);
    let seen = press(&mut rifle, 0, 90);
    assert_eq!(seen.bolts.len(), 1);
    assert_eq!(seen.bolts[0].2, (0.4, 0.5));
    assert_eq!(seen.bolts[0].3.on_hit, on_hit("shield_broken", 8.));
    let (mut great, _) = rig(
        melee(Weapon::Greatblade),
        &[(gun_place(Weapon::Greatblade, 0), "armour_break")],
    );
    let (damage, posture) = light_of(&great);
    let seen = press(&mut great, 0, 90);
    assert_eq!(seen.strikes.len(), 1);
    let strike = &seen.strikes[0].1;
    assert_eq!(strike.reach, reach_m(Weapon::Greatblade));
    assert_eq!(
        (strike.damage, strike.posture),
        (0.4 * damage, 0.5 * posture)
    );
    assert_eq!(strike.on_hit, on_hit("shield_broken", 8.));
}

#[test]
fn sweeping_cut_reads_the_class_in_hand() {
    for weapon in [Weapon::Greatblade, Weapon::Glaive] {
        let (mut actor, _) = rig(melee(weapon), &[(gun_place(weapon, 0), "sweeping_cut")]);
        let (damage, posture) = light_of(&actor);
        let seen = press(&mut actor, 0, 90);
        assert_eq!(seen.strikes.len(), 1, "{weapon:?}");
        let (age, strike) = &seen.strikes[0];
        assert!((age - 0.35).abs() < 0.02, "{age}");
        assert_eq!(
            *strike,
            StrikeBlow {
                reach: reach_m(weapon),
                arc: 110.,
                damage: 1.8 * damage,
                posture,
                on_hit: None,
            },
            "{weapon:?}"
        );
    }
    assert!(reach_m(Weapon::Glaive) > reach_m(Weapon::Greatblade));
    // A class the row is not for does not take it.
    let table = table();
    let row = table.row("sweeping_cut").unwrap();
    assert!(!row.works_with(Weapon::Blade) && !row.works_with(Weapon::Carbine));
}

#[test]
fn a_scaled_strike_reads_the_sheet_and_a_fitted_part_too() {
    let plain = || {
        rig(
            melee(Weapon::Blade),
            &[(gun_place(Weapon::Blade, 0), "disarm")],
        )
        .0
    };
    let (damage, _) = light_of(&plain());
    // A fitted part: a factor on the weapon's damage.
    let mut fitted = plain();
    fitted.equip(&Loadout {
        mods: [(
            Weapon::Blade,
            vec![Factor {
                property: Property::HitDamage,
                factor: 1.5,
            }],
        )]
        .into(),
        ..Loadout::default()
    });
    let (more, _) = light_of(&fitted);
    assert!(more > damage, "{more} {damage}");
    let seen = press(&mut fitted, 0, 60);
    assert_eq!(seen.strikes[0].1.damage, 0.6 * more);
    // The sheet: a box that doubles the light cut.
    let mut graph = SkillGraph::shipped();
    graph.trees[0].novice.effects.push(Effect::Action {
        action: "Light".to_owned(),
        field: "damage".to_owned(),
        op: Op::Mul,
        value: 2.,
    });
    let id = graph.trees[0].novice.id.clone();
    let mut sheeted = plain();
    sheeted.fit(&graph, &sheet_of(&[&id, "blade.novice"]));
    let (doubled, _) = light_of(&sheeted);
    assert_eq!(doubled, 2. * damage);
    sheeted.set_bar(&{
        let mut bar = Bar::default();
        bar.set(gun_place(Weapon::Blade, 0), "disarm");
        bar
    });
    let seen = press(&mut sheeted, 0, 60);
    assert_eq!(seen.strikes[0].1.damage, 0.6 * doubled);
}

#[test]
fn reflect_shot_gives_reflecting_for_eight_seconds() {
    let (mut actor, _) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "reflect_shot")],
    );
    assert_eq!(actor.reflects(), None);
    press(&mut actor, 0, 30);
    assert_eq!(actor.statuses().len(), 1);
    assert_eq!(actor.statuses()[0].0, "reflecting");
    assert_eq!(actor.reflects(), Some(2.));
    let mut input = CombatIntent::default();
    for _ in 0..540 {
        tick(&mut actor, &mut input);
    }
    assert_eq!(actor.reflects(), None, "it lapses");
}

/// The distance and the direction a movement carries its user, summed from
/// the velocities the step writes, and when the first and last were written.
fn carried(
    actor: &mut Combatant,
    index: usize,
    ticks: usize,
    movement: impl Fn(usize) -> PlayerIntent,
) -> (Vec3, Option<f32>, f32, Vec<(f32, StrikeBlow)>) {
    let mut input = CombatIntent {
        ability: Some(index),
        ..CombatIntent::default()
    };
    let (mut sum, mut first, mut last) = (Vec3::ZERO, None, 0.);
    let mut strikes = Vec::new();
    for at in 0..ticks {
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &movement(at), DT, &mut motion);
        if let Some(velocity) = motion.velocity
            && velocity.length() > 0.
        {
            sum += velocity * DT;
            first.get_or_insert(actor.age);
            last = actor.age;
        }
        if let Some(blow) = actor.take_strike() {
            strikes.push((actor.age, blow));
        }
    }
    (sum, first, last, strikes)
}

#[test]
fn thrust_carries_six_metres_along_the_facing_then_strikes_once() {
    let (mut actor, _) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "thrust")],
    );
    let (damage, posture) = light_of(&actor);
    // The yaw is turned after the press: the direction was fixed by it.
    let (sum, first, last, strikes) = carried(&mut actor, 0, 90, |at| PlayerIntent {
        yaw: if at == 0 { 0. } else { 1.2 },
        ..PlayerIntent::default()
    });
    assert!((sum.z + 6.).abs() < 1e-3 && sum.x.abs() < 1e-3, "{sum}");
    assert!(sum.y.abs() < 1e-6);
    let first = first.unwrap();
    assert!(
        first >= 0.15 - 0.02,
        "nothing before the windup ends: {first}"
    );
    assert!(last <= 0.15 + 0.28 + 0.03, "{last}");
    assert_eq!(strikes.len(), 1);
    let (age, strike) = &strikes[0];
    assert!((age - 0.43).abs() < 0.03, "where the movement ends: {age}");
    assert_eq!(
        *strike,
        StrikeBlow {
            reach: reach_m(Weapon::Blade),
            arc: 20.,
            damage: 1.3 * damage,
            posture: 1.2 * posture,
            on_hit: None,
        }
    );
    assert!(actor.cooldown_left("thrust") > 6.);
}

#[test]
fn vault_carries_six_and_a_half_metres_and_strikes_wider_where_it_lands() {
    let (mut actor, _) = rig(
        melee(Weapon::Glaive),
        &[(gun_place(Weapon::Glaive, 0), "vault")],
    );
    let (damage, posture) = light_of(&actor);
    let (sum, first, _, strikes) = carried(&mut actor, 0, 90, |_| PlayerIntent::default());
    assert!((sum.z + 6.5).abs() < 1e-3, "{sum}");
    assert!(first.unwrap() >= 0.10 - 0.02);
    assert_eq!(strikes.len(), 1);
    assert!((strikes[0].0 - 0.65).abs() < 0.03);
    assert_eq!(
        strikes[0].1,
        StrikeBlow {
            reach: reach_m(Weapon::Glaive),
            arc: 60.,
            damage: 1.2 * damage,
            posture: 1.5 * posture,
            on_hit: None,
        }
    );
}

#[test]
fn the_slide_goes_the_way_being_walked_and_fixes_it_at_the_windups_end() {
    let (mut actor, _) = rig(pistol(), &[(Place::Utility(0), "combat_slide")]);
    let index = table().bar.weapon;
    let sideways = PlayerIntent {
        move_dir: Vec2::new(1., 0.),
        ..PlayerIntent::default()
    };
    // Walking right at the press; walking forward afterwards changes nothing.
    let (sum, first, last, strikes) = carried(&mut actor, index, 60, |at| {
        if at < 2 {
            sideways
        } else {
            PlayerIntent {
                move_dir: Vec2::new(0., 1.),
                ..PlayerIntent::default()
            }
        }
    });
    assert!((sum.x - 6.).abs() < 1e-3 && sum.z.abs() < 1e-3, "{sum}");
    assert!(first.unwrap() < 0.05, "it has no windup");
    assert!(last <= 0.45 + 0.03);
    assert!(strikes.is_empty(), "the slide strikes nothing");
    // Standing, it goes the way it faces.
    let (mut standing, _) = rig(
        melee(Weapon::Glaive),
        &[(Place::Utility(0), "combat_slide")],
    );
    let (sum, ..) = carried(&mut standing, index, 60, |_| PlayerIntent::default());
    assert!((sum.z + 6.).abs() < 1e-3 && sum.x.abs() < 1e-3, "{sum}");
}

#[test]
fn armed_lets_the_trigger_fire_during_the_slide_and_nothing_else() {
    let index = table().bar.weapon;
    let during = |id: &str, place: Place, at: usize| {
        let (mut actor, mut input) = rig(Combatant::default(), &[(place, id)]);
        input.ability = Some(at);
        input.fire = true;
        let mut inside = 0;
        let mut after = 0;
        for _ in 0..90 {
            if tick(&mut actor, &mut input) {
                if actor.action == Action::Ability {
                    inside += 1;
                } else {
                    after += 1;
                }
            }
            input.ability = None;
        }
        (inside, after)
    };
    let (inside, after) = during("combat_slide", Place::Utility(0), index);
    assert!(inside >= 1, "the slide keeps the weapon up");
    assert!(after >= 1, "and the trigger works after it");
    let (inside, _) = during("butt_strike", gun_place(Weapon::Carbine, 0), 0);
    assert_eq!(inside, 0, "any other ability holds the trigger");
}

fn walking() -> PlayerIntent {
    PlayerIntent {
        move_dir: Vec2::new(0., 1.),
        ..PlayerIntent::default()
    }
}

fn motion_of(
    actor: &mut Combatant,
    input: &mut CombatIntent,
    movement: &PlayerIntent,
) -> CombatMotion {
    let mut motion = CombatMotion::default();
    actor.step(input, movement, DT, &mut motion);
    motion
}

#[test]
fn slowed_scales_the_pace_and_not_the_travel_of_a_dodge() {
    let mut plain = Combatant::default();
    let mut slowed = Combatant::default();
    slowed.give_status("slowed", 5.);
    let mut input = CombatIntent::default();
    let (a, b) = (
        motion_of(&mut plain, &mut input, &walking()),
        motion_of(&mut slowed, &mut input, &walking()),
    );
    assert_eq!(a.speed_scale, 1.);
    assert_eq!(b.speed_scale, 0.6);
    // Aiming: the aimed pace, times the status.
    let mut aiming = CombatIntent {
        aim: true,
        ..CombatIntent::default()
    };
    let (a, b) = (
        motion_of(&mut plain, &mut aiming, &walking()),
        motion_of(&mut slowed, &mut aiming, &walking()),
    );
    assert_eq!(b.speed_scale, a.speed_scale * 0.6);
    // A dodge's travel is the table's whoever bears what.
    let dodge = |actor: &mut Combatant| {
        let mut input = CombatIntent {
            dodge: true,
            ..CombatIntent::default()
        };
        let mut travelled = Vec3::ZERO;
        for _ in 0..30 {
            let motion = motion_of(actor, &mut input, &walking());
            travelled += motion.velocity.unwrap_or(Vec3::ZERO) * DT;
        }
        travelled
    };
    let (mut one, mut two) = (Combatant::default(), Combatant::default());
    two.give_status("slowed", 5.);
    let (a, b) = (dodge(&mut one), dodge(&mut two));
    assert!(a.length() > 1.);
    assert_eq!(
        a.to_array().map(f32::to_bits),
        b.to_array().map(f32::to_bits)
    );
    // An ability's own pace is scaled by it too.
    let (mut actor, mut input) = rig(
        Combatant::default(),
        &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
    );
    actor.give_status("slowed", 5.);
    input.ability = Some(0);
    let motion = motion_of(&mut actor, &mut input, &walking());
    let motion = motion_of(&mut actor, &mut input, &walking())
        .speed_scale
        .max(motion.speed_scale);
    assert!((motion - 0.3 * 0.6).abs() < 1e-6, "{motion}");
    // It ends: the pace comes back.
    let mut input = CombatIntent::default();
    let mut slowed = Combatant::default();
    slowed.give_status("slowed", 0.5);
    for _ in 0..40 {
        motion_of(&mut slowed, &mut input, &walking());
    }
    assert_eq!(
        motion_of(&mut slowed, &mut input, &walking()).speed_scale,
        1.
    );
}

#[test]
fn shield_broken_holds_the_recovery_and_restarts_the_wait_when_it_ends() {
    let wait = Combatant::default().with_actor(|actor| actor.hurt_wait);
    assert!(wait > 0.);
    let idle = |actor: &mut Combatant, ticks: usize| {
        let mut input = CombatIntent::default();
        for _ in 0..ticks {
            tick(actor, &mut input);
        }
    };
    // Nothing the matter: the shield comes back.
    let mut plain = Combatant::default();
    plain.shield = 0.;
    idle(&mut plain, 30);
    assert!(plain.shield > 0.);
    // Held: it does not, for as long as the status lasts.
    let mut held = Combatant::default();
    held.shield = 0.;
    held.give_status("shield_broken", 1.);
    assert!(held.shield_locked());
    idle(&mut held, 50);
    assert_eq!(held.shield, 0.);
    assert!(held.shield_locked());
    // When it ends the wait starts over: nothing for the wait, then it grows.
    let mut input = CombatIntent::default();
    while held.shield_locked() {
        tick(&mut held, &mut input);
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ticks, a few hundred"
    )]
    idle(&mut held, ((wait - 0.2) / DT) as usize);
    assert_eq!(held.shield, 0., "still waiting");
    idle(&mut held, 120);
    assert!(held.shield > 0., "and then it recovers");
    assert!(!held.shield_locked());
}

#[test]
fn disarmed_forbids_a_shot_a_cut_and_an_ability_and_allows_the_rest() {
    let disarmed = |mut actor: Combatant| {
        actor.give_status("disarmed", 30.);
        assert!(actor.disarmed());
        actor
    };
    let say = Some(Refusal::Ability(AbilityRefusal::Disarmed));
    // A shot, said once.
    let mut gun = disarmed(pistol());
    let ammo = gun.ammo;
    let mut input = CombatIntent {
        fire: true,
        ..CombatIntent::default()
    };
    let mut left = false;
    for _ in 0..60 {
        left |= tick(&mut gun, &mut input);
    }
    assert!(!left && gun.ammo == ammo);
    assert_eq!((gun.refused, gun.refusals), (say, 1));
    // The same actor undisarmed does fire.
    let mut free = pistol();
    let mut input = CombatIntent {
        fire: true,
        ..CombatIntent::default()
    };
    assert!((0..60).any(|_| tick(&mut free, &mut input)));
    // A cut.
    let mut blade = disarmed(melee(Weapon::Blade));
    let mut input = CombatIntent {
        light: true,
        ..CombatIntent::default()
    };
    for _ in 0..10 {
        tick(&mut blade, &mut input);
    }
    assert_eq!((blade.action, blade.refused), (Action::Ready, say));
    let mut input = CombatIntent {
        fire: true,
        ..CombatIntent::default()
    };
    for _ in 0..10 {
        tick(&mut blade, &mut input);
    }
    assert_eq!(blade.action, Action::Ready, "the trigger is a cut too");
    // An ability, which costs nothing.
    let (actor, mut input) = rig(
        Combatant::default(),
        &[(gun_place(Weapon::Carbine, 0), "butt_strike")],
    );
    let mut actor = disarmed(actor);
    let stamina = actor.stamina;
    input.ability = Some(0);
    tick(&mut actor, &mut input);
    assert_eq!(actor.refused, say);
    assert_eq!(
        (actor.action, actor.cooldown_left("butt_strike")),
        (Action::Ready, 0.)
    );
    assert!(actor.stamina >= stamina - 1e-3);
    assert_eq!(
        actor.can_use(0).unwrap_err(),
        AbilityRefusal::Disarmed,
        "a screen is told the same"
    );
    // A dodge, a guard, a reload and a swap.
    let mut dodger = disarmed(Combatant::default());
    let mut input = CombatIntent {
        dodge: true,
        ..CombatIntent::default()
    };
    tick(&mut dodger, &mut input);
    assert_eq!(dodger.action, Action::Dodge);
    let mut guard = disarmed(melee(Weapon::Blade));
    let mut input = CombatIntent {
        aim: true,
        ..CombatIntent::default()
    };
    tick(&mut guard, &mut input);
    assert!(guard.guard_up);
    let mut reloader = disarmed(pistol());
    reloader.ammo = 1;
    let mut input = CombatIntent {
        reload: true,
        ..CombatIntent::default()
    };
    tick(&mut reloader, &mut input);
    assert_eq!(reloader.action, Action::Reload);
    let mut swapper = disarmed(pistol());
    let mut input = CombatIntent {
        weapon: Some(1),
        ..CombatIntent::default()
    };
    tick(&mut swapper, &mut input);
    assert_eq!(swapper.action, Action::Swap);
}

#[test]
fn a_status_a_hit_carries_lands_on_an_open_hit_and_on_no_guarded_one() {
    let blow = || {
        Blow::new(10., 5., Vec3::new(0., 0., -3.), BlowKind::Bolt).with_status(on_hit("slowed", 3.))
    };
    // Open: no guard at all.
    let mut open = Combatant::default();
    assert_eq!(open.hit(blow()).unwrap().guard, GuardOutcome::Open);
    assert_eq!(open.statuses(), [("slowed".to_owned(), 3.)]);
    // A guard up for a while blocks, and carries nothing in.
    let guard = |ticks: usize| {
        let mut actor = melee(Weapon::Blade);
        let mut input = CombatIntent {
            aim: true,
            ..CombatIntent::default()
        };
        for _ in 0..ticks {
            tick(&mut actor, &mut input);
        }
        actor
    };
    let mut blocked = guard(60);
    assert_eq!(blocked.hit(blow()).unwrap().guard, GuardOutcome::Blocked);
    assert!(blocked.statuses().is_empty());
    // Just raised: a parry.
    let mut parried = guard(1);
    assert_eq!(parried.hit(blow()).unwrap().guard, GuardOutcome::Parried);
    assert!(parried.statuses().is_empty());
    // Out of stamina: broken.
    let mut broken = guard(60);
    broken.stamina = 0.;
    assert_eq!(broken.hit(blow()).unwrap().guard, GuardOutcome::Broken);
    assert!(broken.statuses().is_empty());
    // A blow from behind the guard is open to it.
    let mut behind = guard(60);
    let from_behind =
        Blow::new(10., 5., Vec3::new(0., 0., 3.), BlowKind::Bolt).with_status(on_hit("slowed", 3.));
    assert_eq!(behind.hit(from_behind).unwrap().guard, GuardOutcome::Open);
    assert_eq!(behind.statuses().len(), 1);
    // A blow with nothing to carry puts nothing on.
    let mut clean = Combatant::default();
    clean.hit(Blow::new(10., 5., Vec3::new(0., 0., -3.), BlowKind::Bolt));
    assert!(clean.statuses().is_empty());
}

#[test]
fn a_status_given_again_starts_over_and_never_stacks() {
    let mut actor = Combatant::default();
    let blow = || {
        Blow::new(1., 1., Vec3::new(0., 0., -3.), BlowKind::Bolt)
            .with_status(on_hit("disarmed", 3.))
    };
    actor.hit(blow());
    let mut input = CombatIntent::default();
    for _ in 0..60 {
        tick(&mut actor, &mut input);
    }
    let left = actor.statuses()[0].1;
    assert!((left - 2.).abs() < 0.05, "{left}");
    actor.hit(blow());
    assert_eq!(actor.statuses().len(), 1, "one, not two");
    assert_eq!(actor.statuses()[0].1, 3., "its seconds start over");
}

#[test]
fn every_status_ends_at_death() {
    let mut actor = Combatant::default();
    for id in [
        "slowed",
        "shield_broken",
        "disarmed",
        "reflecting",
        "focused",
    ] {
        actor.give_status(id, 30.);
    }
    assert_eq!(actor.statuses().len(), 5);
    actor.hit(Blow::bare(10_000., 0.));
    assert_eq!(actor.action, Action::Dead);
    assert!(actor.statuses().is_empty());
    let mut input = CombatIntent::default();
    tick(&mut actor, &mut input);
    assert!(actor.statuses().is_empty());
    assert!(!actor.disarmed() && !actor.shield_locked() && actor.reflects().is_none());
    // And a hit that kills carries none in.
    let mut dying = Combatant::default();
    dying.health = 1.;
    dying.shield = 0.;
    dying.hit(
        Blow::new(10., 0., Vec3::new(0., 0., -3.), BlowKind::Bolt)
            .with_status(on_hit("slowed", 3.)),
    );
    assert!(dying.statuses().is_empty());
}

#[test]
fn a_breach_empties_the_shield_so_the_whole_blow_is_health() {
    let hit = |shield: f32, breach: bool| {
        let mut actor = Combatant::default();
        actor.shield = shield;
        actor.hit(Blow::new(10., 0., Vec3::new(0., 0., -3.), BlowKind::Bolt).breaching(breach));
        (actor.health, actor.shield)
    };
    let full = Combatant::default().health;
    assert_eq!(hit(30., false), (full, 20.), "the shield takes it");
    assert_eq!(hit(30., true), (full - 10., 0.), "and breached it does not");
    assert_eq!(hit(0., true), hit(0., false), "no shield, no difference");
}

fn candidate(who: u8, at: Vec3, team: u8) -> Candidate<u8> {
    Candidate {
        who,
        at,
        team,
        alive: true,
    }
}

#[test]
fn a_jump_goes_to_the_nearest_other_hostile_and_to_nobody_when_alone() {
    let effects = ShotEffects {
        jump: Some(Jump {
            radius: 8.,
            share: 0.8,
        }),
        ..ShotEffects::default()
    };
    let contact = Vec3::new(0., 0., -10.);
    let first = candidate(1, contact, 1);
    let near = candidate(2, Vec3::new(3., 0., -10.), 1);
    let far = candidate(3, Vec3::new(7., 0., -10.), 1);
    let beyond = candidate(4, Vec3::new(9., 0., -10.), 1);
    let friend = candidate(5, Vec3::new(1., 0., -10.), 0);
    let mut dead = candidate(6, Vec3::new(1.5, 0., -10.), 1);
    dead.alive = false;
    let call = |others: &[Candidate<u8>]| splash(&effects, 0, (10., 20.), Some(1), contact, others);
    let hits = call(&[first, far, near, beyond, friend, dead]);
    assert_eq!(
        hits,
        [Splash {
            who: 2,
            damage: 8.,
            posture: 16.,
            kind: SplashKind::Jump
        }],
        "the nearest other living hostile, with its share, once"
    );
    // Alone: the first is the only one, and it is never struck again.
    assert!(call(&[first]).is_empty());
    assert!(call(&[first, beyond, friend, dead]).is_empty());
    // A bolt that struck nobody (a wall) does not jump.
    assert!(splash(&effects, 0, (10., 20.), None, contact, &[near]).is_empty());
}

#[test]
fn a_blast_strikes_everyone_in_the_radius_once_and_never_the_users_side() {
    let effects = ShotEffects {
        blast: Some(Blast {
            radius: 4.,
            damage: 12.,
            posture: 60.,
        }),
        ..ShotEffects::default()
    };
    let contact = Vec3::new(0., 0., -10.);
    let struck = candidate(1, Vec3::new(0., 0., -10.), 1);
    let beside = candidate(2, Vec3::new(3.9, 0., -10.), 1);
    let outside = candidate(3, Vec3::new(4.1, 0., -10.), 1);
    let friend = candidate(4, Vec3::new(1., 0., -10.), 0);
    let mut dead = candidate(5, Vec3::new(1., 0., -9.), 1);
    dead.alive = false;
    let who = |hits: &[Splash<u8>]| hits.iter().map(|hit| hit.who).collect::<Vec<_>>();
    let hits = splash(
        &effects,
        0,
        (10., 20.),
        Some(1),
        contact,
        &[struck, beside, outside, friend, dead],
    );
    assert_eq!(
        who(&hits),
        [1, 2],
        "the one struck once, the one beside, no more"
    );
    assert!(
        hits.iter()
            .all(|hit| hit.kind == SplashKind::Blast && hit.damage == 12. && hit.posture == 60.),
        "the blast's own figures, the same for everyone in it"
    );
    // At a wall: nobody struck directly, the radius alone.
    let hits = splash(
        &effects,
        0,
        (10., 20.),
        None,
        contact,
        &[beside, outside, friend],
    );
    assert_eq!(who(&hits), [2]);
    // A bolt with nothing on it splashes nobody.
    assert!(
        splash(
            &ShotEffects::default(),
            0,
            (10., 20.),
            Some(1),
            contact,
            &[struck, beside]
        )
        .is_empty()
    );
}

#[test]
fn the_statuses_have_their_rows_and_no_effect_a_table_row_does_not_name() {
    let table = table();
    let slowed = table.status("slowed").unwrap();
    assert_eq!(slowed.pace, Some(0.6));
    assert!(table.status("shield_broken").unwrap().shield_locked);
    assert!(table.status("disarmed").unwrap().disarmed);
    assert_eq!(table.status("reflecting").unwrap().reflects, Some(2.));
    // The first half's two read as they did.
    let focused = table.status("focused").unwrap();
    assert!(focused.tight_cone && focused.pace.is_none() && !focused.disarmed);
    // A bearer of Focused alone is slowed by nothing.
    let mut actor = Combatant::default();
    actor.give_status("focused", 5.);
    let motion = motion_of(&mut actor, &mut CombatIntent::default(), &walking());
    assert_eq!(motion.speed_scale, 1.);
}

#[test]
fn the_slide_lets_no_cut_and_no_second_ability_begin_while_it_lasts() {
    let (mut actor, mut input) = rig(
        melee(Weapon::Blade),
        &[
            (Place::Utility(0), "combat_slide"),
            (gun_place(Weapon::Blade, 0), "thrust"),
        ],
    );
    input.ability = Some(table().bar.weapon);
    tick(&mut actor, &mut input);
    assert_eq!(actor.action, Action::Ability);
    let mut input = CombatIntent {
        light: true,
        fire: true,
        ability: Some(0),
        ..CombatIntent::default()
    };
    for _ in 0..24 {
        tick(&mut actor, &mut input);
        assert_eq!(actor.action, Action::Ability, "still the slide");
        assert_eq!(actor.cooldown_left("thrust"), 0., "no second ability");
        assert!(actor.take_strike().is_none(), "and no cut");
    }
}

#[test]
fn a_volley_already_begun_stops_when_disarmed_lands_on_its_user() {
    let (mut actor, mut input) = rig(
        Combatant::default(),
        &[(gun_place(Weapon::Carbine, 0), "suppressing_fire")],
    );
    input.ability = Some(0);
    let mut shots = 0;
    let mut disarmed = false;
    for _ in 0..120 {
        if tick(&mut actor, &mut input) {
            shots += 1;
        }
        input.ability = None;
        if shots == 3 && !disarmed {
            actor.give_status("disarmed", 5.);
            disarmed = true;
        }
    }
    assert_eq!(shots, 3, "no fourth round after the status");
}

#[test]
fn the_end_of_shield_broken_does_not_stall_the_posture() {
    let mut actor = Combatant::default();
    actor.give_status("shield_broken", 0.5);
    actor.posture = actor.with_actor(|a| a.posture) - 30.;
    let mut input = CombatIntent::default();
    while actor.shield_locked() {
        tick(&mut actor, &mut input);
    }
    let at_end = actor.posture;
    for _ in 0..30 {
        tick(&mut actor, &mut input);
    }
    assert!(actor.posture > at_end, "posture keeps recovering");
}

#[test]
fn the_hit_arc_is_a_table_figure_with_a_check() {
    let text = ABILITIES.replace("hit_arc: 40.0", "hit_arc: 0.0");
    assert_ne!(text, ABILITIES);
    let table: AbilityTable = ron::from_str(&text).expect("still RON");
    assert!(table.check().expect_err("a nought arc").contains("hit_arc"));
}

// Ticket 0010, step 5c: a gun is no guard, a closer covers its user and
// reflect shot costs nothing to hold.

/// What a blow took off `who`: posture, then health and shield together.
fn took(mut who: Combatant, blow: Blow) -> (f32, f32) {
    let (posture, pools) = (who.posture, who.health + who.shield);
    who.hit(blow).expect("it lands");
    (posture - who.posture, pools - (who.health + who.shield))
}

fn gun_bearer() -> Combatant {
    Combatant::default().carrying([Some(Weapon::Carbine), None], 0)
}

fn cut_from_ahead() -> Blow {
    Blow::new(30., 40., Vec3::new(0., 0., -3.), BlowKind::Cut)
}

/// A blow that breaks whoever it lands on, and no more than breaks them.
fn breaking(blow: Blow) -> Blow {
    Blow {
        damage: 1.,
        posture: 500.,
        ..blow
    }
}

#[test]
fn a_caught_break_staggers_longer_and_any_other_break_as_before() {
    let caught = combat_tables::with(|tables| tables.melee.caught);
    assert!(caught.stagger > 1., "the shipped table lengthens it");
    let plain = Combatant::default().spec(Action::Stagger).duration;
    let staggered = |mut who: Combatant, blow: Blow| {
        who.hit(blow).expect("it lands");
        assert_eq!(who.action, Action::Stagger);
        who.duration()
    };
    // The cut of a melee weapon that breaks a gun bearer: the factor.
    let long = staggered(
        gun_bearer(),
        breaking(cut_from_ahead()).made_with_melee(true),
    );
    assert!((long - plain * caught.stagger).abs() < 1e-6, "{long}");
    // A butt strike, a bolt, and a melee cut on a blade bearer: the row.
    let bolt = Blow::new(1., 500., Vec3::new(0., 0., -3.), BlowKind::Bolt);
    for (who, blow) in [
        (gun_bearer(), breaking(cut_from_ahead())),
        (gun_bearer(), bolt),
        (
            Combatant::default().carrying([Some(Weapon::Blade), None], 0),
            breaking(cut_from_ahead()).made_with_melee(true),
        ),
    ] {
        assert_eq!(staggered(who, blow).to_bits(), plain.to_bits());
    }
    // The factor belongs to the stagger it began: the next break, by a
    // bolt, is the row's length again.
    let mut who = gun_bearer();
    who.hit(breaking(cut_from_ahead()).made_with_melee(true))
        .expect("it lands");
    let bolt = Blow::new(1., 500., Vec3::new(0., 0., -3.), BlowKind::Bolt);
    who.hit(bolt).expect("it lands");
    assert_eq!(who.duration().to_bits(), plain.to_bits());
}

#[test]
fn a_cut_of_a_melee_weapon_catches_a_gun_bearer_and_nothing_else_reads_it() {
    let caught = combat_tables::with(|tables| tables.melee.caught);
    // A gun bearer: the factors, on the posture and on the damage.
    let (posture, pools) = took(gun_bearer(), cut_from_ahead().made_with_melee(true));
    assert!((posture - 40. * caught.posture).abs() < 1e-3, "{posture}");
    assert!((pools - 30. * caught.damage).abs() < 1e-3, "{pools}");
    // The same cut not made with a melee weapon (a gun's butt strike) is the
    // blow as it was.
    assert_eq!(
        took(gun_bearer(), cut_from_ahead()),
        (40., 30.),
        "a butt strike is not caught"
    );
    // A melee bearer, at rest or guarding, reads a melee cut to the bit as
    // it read it before.
    let guarding = || {
        let (mut blade, mut input) = rig(melee(Weapon::Blade), &[]);
        input.aim = true;
        for _ in 0..30 {
            tick(&mut blade, &mut input);
        }
        blade
    };
    for who in [melee(Weapon::Blade), melee(Weapon::Glaive), guarding()] {
        let (a, b) = (
            took(who.clone(), cut_from_ahead().made_with_melee(true)),
            took(who, cut_from_ahead()),
        );
        assert_eq!(
            (a.0.to_bits(), a.1.to_bits()),
            (b.0.to_bits(), b.1.to_bits())
        );
    }
    // A bolt on anyone, whoever made it.
    for who in [gun_bearer(), melee(Weapon::Blade)] {
        let bolt = Blow::new(30., 40., Vec3::new(0., 0., -3.), BlowKind::Bolt);
        let (a, b) = (
            took(who.clone(), bolt.clone().made_with_melee(true)),
            took(who, bolt),
        );
        assert_eq!(
            (a.0.to_bits(), a.1.to_bits()),
            (b.0.to_bits(), b.1.to_bits())
        );
    }
}

/// A blade a thrust has begun, `ticks` ticks after the press.
fn thrusting(ticks: usize) -> Combatant {
    let (mut actor, _) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "thrust")],
    );
    let mut input = CombatIntent {
        ability: Some(0),
        ..CombatIntent::default()
    };
    for _ in 0..ticks {
        tick(&mut actor, &mut input);
    }
    actor
}

fn bolt_from(from: Vec3) -> Blow {
    Blow::new(40., 30., from, BlowKind::Bolt)
}

#[test]
fn a_thrusts_cover_blocks_a_bolt_from_ahead_while_it_lasts_and_no_other() {
    let share = 0.8;
    let ahead = Vec3::new(0., 0., -3.);
    // Mid movement (0.15 s windup, 0.28 s movement; tick 18 is about 0.28 s).
    let mut mid = thrusting(18);
    assert_eq!(mid.action, Action::Ability);
    let stamina = mid.stamina;
    let dealt = mid.hit(bolt_from(ahead)).expect("it lands");
    assert_eq!(dealt.guard, GuardOutcome::Blocked);
    assert!((dealt.pools - 40. * (1. - share)).abs() < 1e-3, "{dealt:?}");
    assert_eq!(mid.stamina, stamina, "it costs no stamina");
    assert_eq!(mid.posture, 100., "and all the posture is taken");
    // Before the movement begins and after it ends: open.
    for ticks in [3, 40] {
        let mut actor = thrusting(ticks);
        assert_eq!(actor.action, Action::Ability, "{ticks}");
        let dealt = actor.hit(bolt_from(ahead)).expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Open, "{ticks}");
        assert_eq!(dealt.pools, 40., "{ticks}");
    }
    // From behind and from outside the arc: open.
    for from in [Vec3::new(0., 0., 3.), Vec3::new(3., 0., -0.5)] {
        let dealt = thrusting(18).hit(bolt_from(from)).expect("it lands");
        assert_eq!(dealt.guard, GuardOutcome::Open, "{from}");
        assert_eq!(dealt.pools, 40.);
    }
    // A cut from ahead is not a bolt.
    let dealt = thrusting(18).hit(cut_from_ahead()).expect("it lands");
    assert_eq!(dealt.guard, GuardOutcome::Open);
    assert_eq!(dealt.pools, 30.);
    // A covered bolt carries no status: only an open hit lands one.
    let mut covered = thrusting(18);
    covered.hit(bolt_from(ahead).with_status(on_hit("slowed", 3.)));
    assert!(covered.statuses().is_empty());
    // Reflecting reads the block as it reads a guard's: the two facts the
    // physics asks for a bolt to go back.
    let mut reflecting = thrusting(18);
    reflecting.give_status("reflecting", 3.);
    let dealt = reflecting.hit(bolt_from(ahead)).expect("it lands");
    assert_eq!(dealt.guard, GuardOutcome::Blocked);
    assert_eq!(reflecting.reflects(), Some(2.));
}

#[test]
fn a_cover_outside_its_arc_or_share_is_refused_by_the_check() {
    let refused = |from: &str, to: &str| {
        let text = ABILITIES.replace(from, to);
        assert_ne!(text, ABILITIES, "{from}");
        let table: AbilityTable = ron::from_str(&text).expect("still RON");
        table.check().expect_err(from)
    };
    assert!(
        refused(
            "Some((arc: 60.0, share: 0.8))",
            "Some((arc: 200.0, share: 0.8))"
        )
        .contains("cover")
    );
    assert!(
        refused(
            "Some((arc: 60.0, share: 0.8))",
            "Some((arc: -1.0, share: 0.8))"
        )
        .contains("cover")
    );
    assert!(
        refused(
            "Some((arc: 60.0, share: 0.8))",
            "Some((arc: 60.0, share: 1.5))"
        )
        .contains("cover")
    );
    assert!(
        refused(
            "Some((arc: 60.0, share: 0.8))",
            "Some((arc: 60.0, share: -0.1))"
        )
        .contains("cover")
    );
}

#[test]
fn reflect_shot_is_instant_it_pays_cools_down_and_leaves_the_guard_up() {
    let (mut actor, mut input) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "reflect_shot")],
    );
    let row = table().row("reflect_shot").unwrap().clone();
    assert!(row.is_instant());
    input.aim = true;
    for _ in 0..30 {
        tick(&mut actor, &mut input);
    }
    assert!(actor.guard_up, "the guard is up");
    let stamina = actor.stamina;
    assert_eq!(
        actor.can_use(0).map(|row| row.id),
        Ok("reflect_shot".to_owned())
    );
    input.ability = Some(0);
    tick(&mut actor, &mut input);
    assert_eq!(actor.reflects(), Some(2.), "the status that tick");
    assert_eq!(actor.action, Action::Ready, "it commits to nothing");
    assert!(actor.guard_up, "the guard stays up");
    assert!(actor.stamina < stamina, "it paid");
    assert!(actor.cooldown_left("reflect_shot") > 8.);
    // Refused as any ability is while it cools down.
    assert!(matches!(actor.can_use(0), Err(AbilityRefusal::Cooldown(_))));
    // It may be pressed in the middle of a cut, which it leaves alone.
    let (mut cutting, mut input) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "reflect_shot")],
    );
    input.light = true;
    tick(&mut cutting, &mut input);
    input.light = false;
    assert_eq!(cutting.action, Action::Light);
    input.ability = Some(0);
    tick(&mut cutting, &mut input);
    assert_eq!(cutting.action, Action::Light, "the cut goes on");
    assert_eq!(cutting.reflects(), Some(2.));
    // Short of stamina it is refused like any other.
    let (mut spent, _) = rig(
        melee(Weapon::Blade),
        &[(gun_place(Weapon::Blade, 0), "reflect_shot")],
    );
    spent.stamina = 1.;
    assert!(matches!(spent.can_use(0), Err(AbilityRefusal::Stamina(_))));
}

#[test]
fn only_an_effect_on_the_user_may_have_a_length_of_nought() {
    // The reflect shot's row with a strike for its effect.
    let text = ABILITIES.replace(
        "effect: Own(Status(id: \"reflecting\", seconds: 8.0))",
        "effect: Strike(reach: 1.0, arc: 60.0, damage: 1.0, posture: 1.0, scaled: true)",
    );
    assert_ne!(text, ABILITIES);
    let table: AbilityTable = ron::from_str(&text).expect("still RON");
    let error = table.check().expect_err("a strike of no length");
    assert!(error.contains("instant"), "{error}");
}
