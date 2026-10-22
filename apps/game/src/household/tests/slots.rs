//! A weapon taken from the store into the second slot, drawn, used and
//! saved (ticket 0010, step 2).
#![allow(
    clippy::unwrap_used,
    reason = "a scenario over the real world: a missing piece fails the test"
)]

use super::*;
use bevy::ecs::message::Messages;
use strata::physics::CharacterController;
use strata::sim::items::{Opened, StoreAccess};
use strata::sim::wear::WearCarry;
use strata::sim::{Player, WorldPosition};

const GLAIVE: &str = "veyra:weapon.pier_glaive";

/// What the walk is waiting for.
#[derive(Default)]
enum Step {
    /// The world streamed and the player stood.
    #[default]
    Settle,
    /// The take is asked.
    Take,
    /// The combatant carries what the hands hold.
    Follow,
    /// The glaive is drawn.
    Draw,
    /// The cut is pressed at a dummy a glaive's length away.
    Cut,
    /// The file takes what the sim did.
    Save,
}

#[derive(Default)]
struct Walk {
    step: Step,
    wait: u32,
    dummy: Option<Entity>,
    glaive: Option<Record>,
}

/// The whole walk as one system, a step a tick or so.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one scenario over the sim's own inboxes"
)]
fn walk(
    mut walk: Local<Walk>,
    status: Res<CombatEncounter>,
    registries: Res<strata::sim::Registries>,
    mut store: ResMut<HouseholdStore>,
    mut open: ResMut<OpenStock>,
    mut inbox: ResMut<EquipInbox>,
    mut answers: ResMut<Messages<EquipAnswer>>,
    mut commands: Commands,
    mut players: Query<
        (
            &WorldPosition,
            &Hands,
            &mut CombatIntent,
            &mut Combatant,
            &mut CharacterSheet,
            &WearCarry,
        ),
        With<LocalPlayer>,
    >,
    dummies: Query<&Combatant, Without<LocalPlayer>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok((position, hands, mut input, mut actor, mut sheet, carry)) = players.single_mut() else {
        return;
    };
    if !status.ready {
        return;
    }
    walk.wait += 1;
    let items = &registries.0.items;
    match walk.step {
        Step::Settle if walk.wait > 240 => {
            // A glaive lies in the store, and the store is open to the take.
            let def = items.get(&GLAIVE.parse().unwrap()).unwrap();
            let glaive = strata::sim::crafting::bought(def);
            let cell = store.0.slots().iter().position(Option::is_none).unwrap();
            store.0.set(
                cell,
                Some(strata::sim::items::write(&glaive, items).unwrap()),
            );
            commands.insert_resource(StoreAccess::Anywhere);
            open.0 = Some(Opened {
                menu: slotted_model::MenuId(1),
                with_store: true,
                state: slotted_model::MenuState::new(&slotted_model::MenuDef::new()),
                saddle: slotted_model::Inventory::new(0),
            });
            inbox.push(EquipRequest::Take {
                kind: glaive.clone(),
                hand: 1,
            });
            walk.glaive = Some(glaive);
            walk.step = Step::Take;
            walk.wait = 0;
        }
        Step::Take => {
            let answered: Vec<_> = answers.drain().collect();
            let Some(answer) = answered.first() else {
                return;
            };
            assert!(answer.result.is_ok(), "{:?}", answer.result);
            let saved = hands.saved(items).unwrap();
            assert_eq!(
                saved[1].as_ref().map(|r| r.item.to_string()).as_deref(),
                Some(GLAIVE)
            );
            open.0 = None;
            walk.step = Step::Follow;
            walk.wait = 0;
        }
        Step::Follow => {
            // The slots follow the hands in the tick after the take.
            assert_eq!(actor.slots(), [Some(Weapon::Carbine), Some(Weapon::Glaive)]);
            // The arrival knows the blade; the glaive is Footwork 1's.
            assert!(
                !actor.certified(Weapon::Glaive),
                "uncertified, it is refused"
            );
            sheet.learned.insert("blade.footwork.1".to_owned());
            let fitted = sheet.clone();
            actor.fit(&registries.0.progression, &fitted);
            assert!(actor.certified(Weapon::Glaive));
            input.weapon = Some(1);
            walk.step = Step::Draw;
            walk.wait = 0;
        }
        Step::Draw => {
            assert!(walk.wait < 120, "the glaive is drawn");
            if actor.weapon == Weapon::Glaive && actor.action == Action::Ready {
                walk.step = Step::Cut;
                walk.wait = 0;
            }
        }
        Step::Cut => {
            if walk.dummy.is_none() {
                // Where the player faces, a glaive's length and a little less
                // away: a blade could not reach it.
                let forward = strata::sim::PlayerIntent::forward(actor.facing);
                let ahead = WorldPosition::new(
                    position.0.x() + f64::from(forward.x) * 2.6,
                    position.0.y(),
                    position.0.z() + f64::from(forward.z) * 2.6,
                );
                let tables = strata::sim::combat_tables::current();
                let mut target = Combatant::on_team(1, tables.opponents.passive_health);
                target.damage_scale = 0.;
                walk.dummy = Some(
                    commands
                        .spawn((
                            Player::default(),
                            CharacterController {
                                height: 1.8,
                                responsive: true,
                                ..Default::default()
                            },
                            ahead,
                            target,
                        ))
                        .id(),
                );
                return;
            }
            input.light = walk.wait.is_multiple_of(90);
            let hurt = walk
                .dummy
                .and_then(|dummy| dummies.get(dummy).ok())
                .is_some_and(|dummy| dummy.hurts > 0);
            assert!(walk.wait < 900, "the glaive cut the dummy");
            if hurt {
                assert!(carry.hands[1] > 0, "the cut wore the glaive: {carry:?}");
                assert_eq!(carry.hands[0], 0);
                walk.step = Step::Save;
                walk.wait = 0;
            }
        }
        Step::Save if walk.wait > 60 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// The scenario, in a real headless run: take, draw, cut, and the file.
#[test]
#[allow(clippy::unwrap_used, reason = "a scenario over the real world")]
fn a_glaive_taken_from_the_store_is_drawn_cuts_a_dummy_and_is_saved() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    let mut cli = crate::Cli::parse_from([
        "game",
        "--headless",
        "--no-hostiles",
        "--fresh",
        "--household",
        path.to_str().unwrap(),
    ]);
    cli.resolve_play_mode();
    crate::install_combat_tables(&cli).unwrap();
    let mut app = crate::build(cli, None).unwrap();
    app.add_systems(
        strata::sim::SimTick,
        walk.in_set(strata::sim::SimSet::Input)
            .after(strata::sim::equip::consume),
    );
    let exit = app.run();
    assert_eq!(exit, AppExit::Success);

    let saved: Household = ron::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let registries = shipped();
    let arrival = &saved.characters[0];
    let families: Vec<_> = arrival
        .hands
        .iter()
        .map(|hand| hand.as_ref().map(|record| record.item.to_string()))
        .collect();
    assert_eq!(
        families,
        [
            Some("veyra:weapon.patrol_carbine".to_owned()),
            Some(GLAIVE.to_owned())
        ]
    );
    assert_eq!(registries.items.check_hands(&arrival.hands), Ok(()));
    // The pistol lies where the glaive was, in the store.
    assert!(
        saved
            .store
            .iter()
            .flatten()
            .any(|record| record.item.to_string() == "veyra:weapon.service_pistol")
    );
    assert!(
        !saved
            .store
            .iter()
            .flatten()
            .any(|record| record.item.to_string() == GLAIVE)
    );
}
