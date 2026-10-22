//! The adapter half of the `combat` rules: its requests, systems and plugin.

pub use veyra_rules::combat::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use bevy::time::{Fixed, Time};

use crate::{PlayerCommand, SimSet, SimTick};

/// Installed even headless; an empty encounter does no work.
#[derive(Debug, Clone, Copy)]
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AttackRequest>()
            .add_message::<CombatImpact>()
            .add_message::<ShotFired>()
            .add_systems(SimTick, tick.in_set(CombatSet::Decide).in_set(SimSet::Step))
            .add_systems(
                SimTick,
                finish_contacts
                    .in_set(CombatSet::Finish)
                    .in_set(SimSet::Observe),
            );
    }
}

fn finish_contacts(mut actors: Query<(&mut Combatant, &PlayerCommand)>) {
    for (mut actor, movement) in &mut actors {
        actor.finish_contacts(&movement.0);
    }
}

fn tick(
    time: Res<Time<Fixed>>,
    mut attacks: MessageWriter<AttackRequest>,
    mut actors: Query<(
        Entity,
        &mut Combatant,
        &mut CombatIntent,
        &mut CombatMotion,
        &PlayerCommand,
    )>,
) {
    for (entity, mut actor, mut input, mut motion, movement) in &mut actors {
        let (contact, shot) = actor.step(&mut input, &movement.0, time.delta_secs(), &mut motion);
        if let Some((from, to)) = contact {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.sequence,
                kind: AttackKind::Melee {
                    action: actor.action,
                    from,
                    to,
                },
            });
        }
        if shot {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.shots,
                kind: AttackKind::Shot(input.aim_point),
            });
        }
        if let Some(blow) = actor.take_strike() {
            attacks.write(AttackRequest {
                attacker: entity,
                sequence: actor.sequence,
                kind: AttackKind::Strike {
                    reach: blow.reach,
                    arc: blow.arc,
                    damage: blow.damage,
                    posture: blow.posture,
                    on_hit: blow.on_hit,
                },
            });
        }
    }
}
