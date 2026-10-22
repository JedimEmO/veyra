//! The scripted fight: `--combat-script` and `--script-progression` drive the
//! player through the encounter on a timetable, which the development flags
//! turn on. A child of the encounter's module, whose state it reads.

#![allow(
    clippy::wildcard_imports,
    reason = "a child of the encounter's module, which it reads whole"
)]

use super::*;

#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "one script, phase by phase, the way the capture was timed; it reads the encounter, the graph and the ground"
)]
pub(super) fn drive_script(
    time: Res<Time<Fixed>>,
    mut encounter: ResMut<Encounter>,
    script: Option<Res<veyra_dev::household::ProgressionScript>>,
    guard_hold: Option<Res<veyra_game::GuardHold>>,
    registries: Res<strata::sim::Registries>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    mut players: Query<
        (
            Entity,
            &WorldPosition,
            &mut ScriptedIntent,
            &mut CombatIntent,
            &Combatant,
            &CharacterSheet,
        ),
        With<LocalPlayer>,
    >,
    targets: Query<
        (&WorldPosition, &Opponent, &Combatant, &CharacterController),
        Without<LocalPlayer>,
    >,
) {
    if !encounter.started || !encounter.scripted {
        return;
    }
    let before = encounter.elapsed - 5.;
    encounter.elapsed += time.delta_secs();
    let now = encounter.elapsed - 5.;
    // Once the fight is won the household script walks the player.
    if script.is_some_and(|s| !s.fighting()) {
        return;
    }
    let hold_guard = guard_hold.is_some_and(|hold| hold.0);
    for (entity, position, mut movement, mut input, actor, sheet) in &mut players {
        movement.0 = PlayerIntent::default();
        // A draw request is held, not an edge: it survives the per-tick reset
        // the way a player's does until the sim takes it.
        let weapon = input.weapon;
        // `--hold-guard` writes the button once a frame, and the reset below
        // would take it before the sim does; its own clock says whether it
        // is holding now, so the script's output never feeds the latch.
        let held_aim = hold_guard;
        *input = CombatIntent::default();
        input.weapon = weapon;
        let target = targets
            .iter()
            .find(|(_, enemy, _, _)| enemy.passive)
            .map_or(
                position.0.0 + bevy::math::DVec3::NEG_Z * 100.,
                |(position, _, _, _)| position.0.0,
            );
        input.aim_point = WorldPos(target + bevy::math::DVec3::Y * 0.2);
        let delta = target - position.0.0;
        #[allow(clippy::cast_possible_truncation, reason = "local scripted heading")]
        {
            movement.0.yaw = (-delta.x).atan2(-delta.z) as f32;
        }
        // After the moveset study, fight both hostiles with ordinary inputs.
        // This exercises the real encounter without immunity or damage overrides.
        if now >= 17. {
            let mut target = targets
                .iter()
                .filter(|(_, enemy, actor, _)| !enemy.passive && actor.action != Action::Dead)
                .min_by(|(a, _, _, _), (b, _, _, _)| {
                    a.0.distance(position.0)
                        .total_cmp(&b.0.distance(position.0))
                });
            // Hostiles down and the lesson still unpaid: the training
            // target takes the rest of the cuts.
            if target.is_none()
                && encounter.progression
                && veyra_dev::household::short_of_gate(sheet, &registries.0.progression)
            {
                target = targets.iter().find(|(_, enemy, _, _)| enemy.passive);
            }
            if let Some((target, enemy, _, motor)) = target {
                let distance = target.0.distance(position.0);
                let lead = motor.horizontal_velocity.as_dvec3() * (distance / 38.).min(0.6);
                input.aim_point = WorldPos(target.0.0 + lead + bevy::math::DVec3::Y * 0.2);
                let aim = input.aim_point.0 - position.0.0;
                #[allow(clippy::cast_possible_truncation, reason = "local scripted heading")]
                {
                    movement.0.yaw = (-aim.x).atan2(-aim.z) as f32;
                }
                // The rusher is taken with the blade, the sentry with the
                // carbine; the draw is held until the sim takes it.
                let wanted = if enemy.brain.kind.ranged() {
                    Weapon::Carbine
                } else {
                    Weapon::Blade
                };
                if actor.weapon != wanted
                    && let Some(slot) = actor.slots().iter().position(|slot| *slot == Some(wanted))
                {
                    input.weapon = Some(slot);
                }
                // The progression script takes the rusher with the blade:
                // close in and cut whenever the body is free, so the kill
                // earns blade XP and not ranged.
                if encounter.progression && !enemy.brain.kind.ranged() {
                    if distance > 1.5 {
                        movement.0.move_dir.y = 1.;
                        movement.0.sprint = distance > 6.;
                    } else {
                        input.light = actor.action == Action::Ready;
                    }
                    continue;
                }
                // A scripted hand taps: a pistol fires a press at a time and
                // a carbine a burst, so a held trigger would fire the pistol
                // once. The pulse is slow enough for every press to land.
                input.fire = !(18.0..21.0).contains(&now) && (now * 20.).floor() % 2. == 0.;
                input.aim = true;
                input.reload = actor.ammo == 0;
                movement.0.move_dir.x = if (now % 6.) < 3. { 0.6 } else { -0.6 };
                if distance < 5. {
                    movement.0.move_dir.y = -0.5;
                } else {
                    let shoulder = origin.to_render(position.0) + Vec3::Y * 0.45;
                    let path = origin.to_render(input.aim_point) - shoulder;
                    let blocked = Dir3::new(path).is_ok_and(|direction| {
                        spatial
                            .cast_ray(
                                shoulder,
                                direction,
                                (path.length() - 0.8).max(0.),
                                true,
                                &SpatialQueryFilter::from_excluded_entities([entity]),
                            )
                            .is_some()
                    });
                    if blocked || distance > 22. {
                        movement.0.move_dir.y = 0.8;
                    }
                }
            }
            continue;
        }
        // Windows leave enough time for streamed colliders and the first asset load.
        input.fire =
            (3.0..4.0).contains(&now) || (12.0..14.0).contains(&now) || (15.6..16.6).contains(&now);
        input.aim = input.fire || held_aim;
        // A cut is a blade's, a burst a gun's: draw the blade for the cuts
        // and the carbine for the shots that follow them. The fight that
        // starts at 17 s draws what each target wants.
        let slot_of = |weapon| actor.slots().iter().position(|slot| *slot == Some(weapon));
        if before < 4.2 && now >= 4.2 {
            input.weapon = slot_of(Weapon::Blade);
        }
        if before < 10.8 && now >= 10.8 {
            input.weapon = slot_of(Weapon::Carbine);
        }
        if (4.0..5.0).contains(&now) {
            movement.0.move_dir.y = 1.;
        }
        input.light = [5.2, 5.38, 10.0].iter().any(|t| before < *t && now >= *t);
        input.heavy = before < 7.0 && now >= 7.0;
        input.dodge = before < 8.4 && now >= 8.4;
        if (8.3..8.5).contains(&now) {
            movement.0.move_dir.x = 1.;
        }
        input.reload = before < 14.2 && now >= 14.2;
    }
}

pub(super) fn scripted_camera(
    players: Query<&ScriptedIntent, With<LocalPlayer>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    if let Ok(input) = players.single() {
        for mut camera in &mut cameras {
            camera.yaw = input.0.yaw;
        }
    }
}
