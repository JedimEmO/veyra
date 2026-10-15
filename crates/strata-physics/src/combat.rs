//! Collision-resolved combat: nearest muzzle obstruction and swept melee arcs.

use std::collections::HashMap;

use crate::CharacterController;
use avian3d::prelude::{Collider, ShapeCastConfig, SpatialQuery, SpatialQueryFilter};
use bevy::ecs::prelude::*;
use bevy::math::{Dir3, Quat, Vec3};
use bevy::time::{Fixed, Time};
use strata_sim::ability::{Candidate, ShotEffects, Splash, SplashKind, splash};
use strata_sim::combat::{
    Action, ActionSpec, AttackKind, AttackRequest, BlasterBolt, Blow, BlowKind, CombatImpact,
    Combatant, GuardOutcome, ShotFired, cut_clip_time, deviate, shot_draw,
};
use strata_sim::combat_curves::MeleeCurves;
use strata_sim::{PlayerIntent, RenderOrigin, WorldPosition};

/// Each target receives at most one contact from an action sequence.
#[derive(Default)]
pub struct ContactLedger(HashMap<Entity, (u64, Vec<Entity>)>);

impl ContactLedger {
    fn accept(&mut self, attacker: Entity, sequence: u64, victim: Entity) -> bool {
        let (seen_sequence, targets) = self.0.entry(attacker).or_insert((sequence, Vec::new()));
        if *seen_sequence != sequence {
            *seen_sequence = sequence;
            targets.clear();
        }
        if targets.contains(&victim) {
            return false;
        }
        targets.push(victim);
        true
    }
}

/// A shoulder-height blade arc, sampled by the server independently of a
/// mesh. `spec` is the attacker's own row for `action`, whose contact
/// interval the arc spans.
#[must_use]
pub fn blade_point(action: Action, spec: &ActionSpec, time: f32, reach: f32) -> Vec3 {
    let u = ((time - spec.contact_start) / (spec.contact_end - spec.contact_start)).clamp(0., 1.);
    if action == Action::Heavy {
        let angle = (-55.0 + 105.0 * u).to_radians();
        Vec3::new(0.12, 0.35 - angle.sin() * reach, -angle.cos() * reach)
    } else {
        let direction = if action == Action::Followup {
            -1.0
        } else {
            1.0
        };
        let angle = direction * (-70.0 + 140.0 * u).to_radians();
        Vec3::new(angle.sin() * reach, 0.2, -angle.cos() * reach)
    }
}

/// All contacts use the physics world's colliders, including scenery and trees.
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "the shot and swept melee branches share one validated attacker"
)]
pub fn resolve_attacks(
    mut commands: Commands,
    mut fired: MessageWriter<ShotFired>,
    mut requests: MessageReader<AttackRequest>,
    mut impacts: MessageWriter<CombatImpact>,
    spatial: SpatialQuery,
    origin: Res<RenderOrigin>,
    positions: Query<(&WorldPosition, &CharacterController)>,
    bodies: Query<(Entity, &WorldPosition), With<Combatant>>,
    curves: Option<Res<MeleeCurves>>,
    mut actors: Query<&mut Combatant>,
    mut ledger: Local<ContactLedger>,
) {
    ledger.0.retain(|entity, _| actors.contains(*entity));
    for request in requests.read() {
        let Ok((position, controller)) = positions.get(request.attacker) else {
            continue;
        };
        let Ok(attacker) = actors.get(request.attacker) else {
            continue;
        };
        if attacker.action == Action::Dead {
            continue;
        }
        let (team, facing, power, speed, shot, cone, weapon, salt, factors, effects) = (
            attacker.team,
            attacker.facing,
            attacker.damage_scale,
            attacker.bolt_speed,
            attacker.weapon_spec(),
            attacker.shot_cone(),
            attacker.weapon,
            attacker.shot_salt,
            attacker.shot_factors(),
            attacker.shot_effects().clone(),
        );
        // The shooter's scale is on a blast once, here, whether the bolt
        // flies or bursts at a blocked barrel.
        let effects = {
            let mut effects = effects;
            if let Some(blast) = &mut effects.blast {
                blast.damage *= power;
                blast.posture *= power;
            }
            effects
        };
        let root = origin.to_render(position.0);
        let filter = SpatialQueryFilter::from_excluded_entities([request.attacker]);
        match request.kind.clone() {
            AttackKind::Shot(target) => {
                let shoulder = root + Vec3::Y * 0.45;
                let muzzle = shoulder
                    + PlayerIntent::forward(facing) * 0.45
                    + PlayerIntent::right(facing) * 0.2;
                let aim = origin.to_render(target) - muzzle;
                if !aim.is_finite() {
                    continue;
                }
                let Ok(direction) = Dir3::new(aim) else {
                    continue;
                };
                // A barrel can protrude through thin cover. A blocked barrel
                // dissipates at that cover immediately, never on the far side.
                let barrel = muzzle - shoulder;
                let barrel_direction = Dir3::new_unchecked(barrel.normalize());
                if let Some(hit) =
                    spatial.cast_ray(shoulder, barrel_direction, barrel.length(), true, &filter)
                {
                    fired.write(ShotFired {
                        attacker: request.attacker,
                        from: origin.to_world(shoulder),
                        direction: *barrel_direction,
                    });
                    let victim_root = positions
                        .get(hit.entity)
                        .map_or(root, |(at, _)| origin.to_render(at.0));
                    let dealt = actors.get_mut(hit.entity).ok().and_then(|mut actor| {
                        (actor.team != team)
                            .then(|| {
                                // A blocked barrel's point blank hit comes
                                // from the shooter.
                                actor.hit(
                                    Blow::new(
                                        shot.damage * power * factors.0,
                                        shot.posture * power * factors.1,
                                        root - victim_root,
                                        BlowKind::Bolt,
                                    )
                                    .with_status(effects.on_hit.clone())
                                    .breaching(effects.breach),
                                )
                            })
                            .flatten()
                    });
                    impacts.write(CombatImpact {
                        attacker: request.attacker,
                        victim: dealt.map(|_| hit.entity),
                        from: origin.to_world(shoulder),
                        point: origin.to_world(shoulder + barrel_direction * hit.distance),
                        melee: false,
                        dealt: dealt.map_or(0., |d| d.pools),
                        lethal: dealt.is_some_and(|d| d.lethal),
                        guard: dealt.map_or(GuardOutcome::Open, |d| d.guard),
                        shot: None,
                    });
                    let contact = shoulder + barrel_direction * hit.distance;
                    splash_hits(
                        &Landing {
                            effects: &effects,
                            attacker: request.attacker,
                            team,
                            weapon,
                            flown: 0.,
                            figures: (
                                shot.damage * power * factors.0,
                                shot.posture * power * factors.1,
                            ),
                            struck: dealt
                                .filter(|d| d.guard != GuardOutcome::Parried)
                                .map(|_| hit.entity),
                            contact,
                        },
                        &origin,
                        &spatial,
                        &bodies,
                        &mut actors,
                        &mut impacts,
                    );
                    // Point blank there is no bolt to send back: the share
                    // lands on the shooter at once.
                    if let Some(returned) = reflected_share(&actors, hit.entity, dealt)
                        && let Ok(mut shooter) = actors.get_mut(request.attacker)
                    {
                        let sent = shooter.hit(Blow::new(
                            shot.damage * power * factors.0 * returned,
                            shot.posture * power * factors.1 * returned,
                            victim_root - root,
                            BlowKind::Bolt,
                        ));
                        impacts.write(CombatImpact {
                            attacker: hit.entity,
                            victim: sent.map(|_| request.attacker),
                            from: origin.to_world(contact),
                            point: origin.to_world(shoulder),
                            melee: false,
                            dealt: sent.map_or(0., |d| d.pools),
                            lethal: sent.is_some_and(|d| d.lethal),
                            guard: sent.map_or(GuardOutcome::Open, |d| d.guard),
                            shot: None,
                        });
                    }
                    continue;
                }
                // The request carries the shooter's count of shots, so the
                // draw is the same on a server and in a replayed bout.
                let direction = Dir3::new(deviate(
                    *direction,
                    cone,
                    shot_draw(request.sequence.wrapping_add(salt), team, weapon),
                ))
                .unwrap_or(direction);
                let position = origin.to_world(muzzle);
                fired.write(ShotFired {
                    attacker: request.attacker,
                    from: position,
                    direction: *direction,
                });
                commands.spawn(BlasterBolt {
                    owner: request.attacker,
                    team,
                    previous: position,
                    position,
                    velocity: *direction * speed,
                    damage: shot.damage * power * factors.0,
                    posture: shot.posture * power * factors.1,
                    remaining: shot.range,
                    flown: 0.,
                    falloff: shot.falloff,
                    weapon,
                    effects: effects.clone(),
                });
            }
            AttackKind::Strike {
                reach,
                arc,
                damage,
                posture,
                on_hit,
            } => {
                if attacker.sequence != request.sequence {
                    continue;
                }
                let forward = PlayerIntent::forward(facing);
                let shoulder = root + Vec3::Y * 0.25;
                let half = arc.to_radians().cos();
                let blow_from = |victim_root: Vec3| root - victim_root;
                for (victim_entity, at) in &bodies {
                    let victim_root = origin.to_render(at.0);
                    let offset = victim_root - root;
                    let flat = Vec3::new(offset.x, 0., offset.z);
                    // Reach and arc on the ground plane, with the same reach
                    // as the height a blow can span.
                    if victim_entity == request.attacker
                        || flat.length() > reach
                        || offset.y.abs() > reach
                    {
                        continue;
                    }
                    if flat.try_normalize().is_some_and(|d| forward.dot(d) < half) {
                        continue;
                    }
                    let path = victim_root - shoulder;
                    if let Ok(sight) = Dir3::new(path)
                        && spatial
                            .cast_ray(shoulder, sight, path.length(), true, &filter)
                            .is_some_and(|block| block.entity != victim_entity)
                    {
                        continue;
                    }
                    let Ok(mut victim) = actors.get_mut(victim_entity) else {
                        continue;
                    };
                    if victim.team == team
                        || !ledger.accept(request.attacker, request.sequence, victim_entity)
                    {
                        continue;
                    }
                    let blow = Blow::new(
                        damage * power,
                        posture * power,
                        blow_from(victim_root),
                        BlowKind::Cut,
                    )
                    .with_status(on_hit.clone())
                    .made_with_melee(weapon.is_melee());
                    if let Some(dealt) = victim.hit(blow) {
                        if dealt.riposte > 0.
                            && let Ok(mut swinger) = actors.get_mut(request.attacker)
                        {
                            swinger.riposted(dealt.riposte);
                        }
                        impacts.write(CombatImpact {
                            attacker: request.attacker,
                            victim: Some(victim_entity),
                            from: origin.to_world(shoulder),
                            point: at.0,
                            melee: true,
                            dealt: dealt.pools,
                            lethal: dealt.lethal,
                            guard: dealt.guard,
                            shot: None,
                        });
                    }
                }
            }
            AttackKind::Melee { action, from, to } => {
                if attacker.sequence != request.sequence || attacker.action != action {
                    continue;
                }
                let rotation = Quat::from_rotation_y(facing);
                let spec = attacker.spec(action);
                let (reach_factor, clip) =
                    (attacker.reach_factor(), attacker.clip_duration(action));
                let recovery = attacker.recovery_factor();
                // Eight short segments bound angular sweep error even if a tick
                // spans the whole contact window. Radial samples include the hilt.
                for segment in 0..8_u8 {
                    let a = from + (to - from) * f32::from(segment) / 8.;
                    let b = from + (to - from) * f32::from(segment + 1) / 8.;
                    for reach in [0., 0.33, 0.67, 1.] {
                        let local = |time| {
                            curves.as_ref().map_or_else(
                                || {
                                    blade_point(
                                        action,
                                        &spec,
                                        time,
                                        0.45 + reach * reach_factor * 1.2,
                                    )
                                },
                                |curves| {
                                    let time = cut_clip_time(time, spec.contact_end, recovery);
                                    curves.point(action, time, clip, reach_factor, reach)
                                        - Vec3::Y * controller.half_height()
                                },
                            )
                        };
                        let start = root + rotation * local(a);
                        let end = root + rotation * local(b);
                        let travel = end - start;
                        let Ok(direction) = Dir3::new(travel) else {
                            continue;
                        };
                        let Some(hit) = spatial.cast_shape(
                            &Collider::sphere(0.23),
                            start,
                            Quat::IDENTITY,
                            direction,
                            &ShapeCastConfig::from_max_distance(travel.length()),
                            &filter,
                        ) else {
                            continue;
                        };
                        if !actors.contains(hit.entity) {
                            continue;
                        }
                        let point = start + direction * hit.distance;
                        let shoulder = root + Vec3::Y * 0.25;
                        let path = point - shoulder;
                        let Ok(sight) = Dir3::new(path) else {
                            continue;
                        };
                        if spatial
                            .cast_ray(shoulder, sight, path.length(), true, &filter)
                            .is_some_and(|block| block.entity != hit.entity)
                        {
                            continue;
                        }
                        let Ok(mut victim) = actors.get_mut(hit.entity) else {
                            continue;
                        };
                        if victim.team == team
                            || !ledger.accept(request.attacker, request.sequence, hit.entity)
                        {
                            continue;
                        }
                        let victim_root = positions
                            .get(hit.entity)
                            .map_or(root, |(at, _)| origin.to_render(at.0));
                        let blow = Blow::new(
                            spec.damage * power,
                            spec.posture * power,
                            root - victim_root,
                            BlowKind::Cut,
                        )
                        .made_with_melee(weapon.is_melee());
                        if let Some(dealt) = victim.hit(blow) {
                            // A parried cut costs its swinger posture, which
                            // may stagger them and end the swing.
                            if dealt.riposte > 0.
                                && let Ok(mut swinger) = actors.get_mut(request.attacker)
                            {
                                swinger.riposted(dealt.riposte);
                            }
                            impacts.write(CombatImpact {
                                attacker: request.attacker,
                                victim: Some(hit.entity),
                                from: origin.to_world(shoulder),
                                point: origin.to_world(point),
                                melee: true,
                                dealt: dealt.pools,
                                lethal: dealt.lethal,
                                guard: dealt.guard,
                                shot: None,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Where a bolt first made contact and what it brings: everything the
/// splash needs of it.
struct Landing<'a> {
    effects: &'a ShotEffects,
    attacker: Entity,
    team: u8,
    weapon: strata_sim::combat::Weapon,
    flown: f32,
    /// Damage and posture at contact.
    figures: (f32, f32),
    /// The actor the bolt landed on, unless its guard turned the bolt aside.
    struck: Option<Entity>,
    /// Render space.
    contact: Vec3,
}

/// The share a guard sends back when it blocked or parried a bolt and bears
/// Reflecting: the one test both landings of a bolt (in flight and at a
/// blocked barrel) ask.
fn reflected_share(
    actors: &Query<&mut Combatant>,
    guard: Entity,
    dealt: Option<strata_sim::combat::Dealt>,
) -> Option<f32> {
    let dealt = dealt?;
    matches!(dealt.guard, GuardOutcome::Blocked | GuardOutcome::Parried)
        .then(|| actors.get(guard).ok()?.reflects())
        .flatten()
}

/// Land the blast and the jump of a bolt's first contact: the sim's
/// [`splash`] says who is struck and with what, this asks it, checks that a
/// jump has a line to its target and lands the blows.
fn splash_hits(
    landing: &Landing,
    origin: &RenderOrigin,
    spatial: &SpatialQuery,
    bodies: &Query<(Entity, &WorldPosition), With<Combatant>>,
    actors: &mut Query<&mut Combatant>,
    impacts: &mut MessageWriter<CombatImpact>,
) {
    if landing.effects.blast.is_none() && landing.effects.jump.is_none() {
        return;
    }
    let others: Vec<_> = bodies
        .iter()
        .filter_map(|(who, at)| {
            let actor = actors.get(who).ok()?;
            Some(Candidate {
                who,
                at: origin.to_render(at.0),
                team: actor.team,
                alive: actor.action != Action::Dead && actor.health > 0.,
            })
        })
        .collect();
    let hits = splash(
        landing.effects,
        landing.team,
        landing.figures,
        landing.struck,
        landing.contact,
        &others,
    );
    for Splash {
        who,
        damage,
        posture,
        kind,
    } in hits
    {
        let Some(at) = others.iter().find(|o| o.who == who).map(|o| o.at) else {
            continue;
        };
        if kind == SplashKind::Jump {
            let path = at - landing.contact;
            let filter = SpatialQueryFilter::from_excluded_entities(
                [Some(landing.attacker), landing.struck]
                    .into_iter()
                    .flatten(),
            );
            if let Ok(sight) = Dir3::new(path)
                && spatial
                    .cast_ray(landing.contact, sight, path.length(), true, &filter)
                    .is_some_and(|block| block.entity != who)
            {
                continue;
            }
        }
        let Ok(mut victim) = actors.get_mut(who) else {
            continue;
        };
        // A blast comes off the ground beside the victim, down no line of
        // fire, so no guard meets it; a jump is a bolt flying on.
        let blow = match kind {
            SplashKind::Blast => Blow::blast(damage, posture),
            SplashKind::Jump => Blow::new(damage, posture, landing.contact - at, BlowKind::Bolt),
        };
        let dealt = victim.hit(blow);
        impacts.write(CombatImpact {
            attacker: landing.attacker,
            victim: dealt.map(|_| who),
            from: origin.to_world(landing.contact),
            point: origin.to_world(at),
            melee: false,
            dealt: dealt.map_or(0., |d| d.pools),
            lethal: dealt.is_some_and(|d| d.lethal),
            guard: dealt.map_or(GuardOutcome::Open, |d| d.guard),
            shot: Some((landing.weapon, landing.flown)),
        });
    }
}

/// Swept collision prevents tunnelling; every bolt flies toward its launch aim.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "a system's queries, and the hit, the splash and the return of one bolt in order"
)]
pub fn advance_bolts(
    mut commands: Commands,
    time: Res<Time<Fixed>>,
    spatial: SpatialQuery,
    origin: Res<RenderOrigin>,
    mut bolts: Query<(Entity, &mut BlasterBolt)>,
    bodies: Query<(Entity, &WorldPosition), With<Combatant>>,
    mut actors: Query<&mut Combatant>,
    mut impacts: MessageWriter<CombatImpact>,
) {
    for (entity, mut bolt) in &mut bolts {
        let Ok(direction) = Dir3::new(bolt.velocity) else {
            commands.entity(entity).despawn();
            continue;
        };
        let distance = (bolt.velocity.length() * time.delta_secs()).min(bolt.remaining);
        let start = origin.to_render(bolt.position);
        let filter = SpatialQueryFilter::from_excluded_entities([bolt.owner]);
        let hit = spatial.cast_shape(
            &Collider::sphere(0.07),
            start,
            Quat::IDENTITY,
            direction,
            &ShapeCastConfig::from_max_distance(distance),
            &filter,
        );
        // Parry's sphere cast can return nothing for a bolt flying exactly
        // down the line through a capsule's middle (an exact shot at a
        // standing body), so a ray over the same stretch catches what it
        // lets through.
        let hit = hit.map(|hit| (hit.distance, hit.entity)).or_else(|| {
            spatial
                .cast_ray(start, direction, distance, true, &filter)
                .map(|hit| (hit.distance, hit.entity))
        });
        let travelled = hit.map_or(distance, |(travelled, _)| travelled);
        bolt.previous = bolt.position;
        bolt.position.0 += (direction * travelled).as_dvec3();
        bolt.remaining -= travelled;
        bolt.flown += travelled;
        let share = bolt.falloff.share_at(bolt.flown);
        if let Some((_, victim)) = hit {
            let dealt = actors.get_mut(victim).ok().and_then(|mut actor| {
                (actor.team != bolt.team)
                    .then(|| {
                        let from = -bolt.velocity;
                        actor.hit(
                            Blow::new(
                                bolt.damage * share,
                                bolt.posture * share,
                                from,
                                BlowKind::Bolt,
                            )
                            .with_status(bolt.effects.on_hit.clone())
                            .breaching(bolt.effects.breach),
                        )
                    })
                    .flatten()
            });
            impacts.write(CombatImpact {
                attacker: bolt.owner,
                victim: dealt.map(|_| victim),
                from: bolt.previous,
                point: bolt.position,
                melee: false,
                dealt: dealt.map_or(0., |d| d.pools),
                lethal: dealt.is_some_and(|d| d.lethal),
                guard: dealt.map_or(GuardOutcome::Open, |d| d.guard),
                shot: Some((bolt.weapon, bolt.flown)),
            });
            splash_hits(
                &Landing {
                    effects: &bolt.effects,
                    attacker: bolt.owner,
                    team: bolt.team,
                    weapon: bolt.weapon,
                    flown: bolt.flown,
                    figures: (bolt.damage * share, bolt.posture * share),
                    struck: dealt
                        .filter(|d| d.guard != GuardOutcome::Parried)
                        .map(|_| victim),
                    contact: start + direction * travelled,
                },
                &origin,
                &spatial,
                &bodies,
                &mut actors,
                &mut impacts,
            );
            if !bolt.effects.reflected
                && let Some(returned) = reflected_share(&actors, victim, dealt)
                && let Ok(guard) = actors.get(victim)
                && let Ok((_, owner_at)) = bodies.get(bolt.owner)
            {
                // The bolt goes back from where it met the guard, at the
                // one who fired it, as the guard's own.
                let back =
                    origin.to_render(owner_at.0) + Vec3::Y * 0.45 - (start + direction * travelled);
                if let Ok(toward) = Dir3::new(back) {
                    commands.spawn(BlasterBolt {
                        owner: victim,
                        team: guard.team,
                        previous: bolt.position,
                        position: bolt.position,
                        velocity: *toward * bolt.velocity.length(),
                        damage: bolt.damage * share * returned,
                        posture: bolt.posture * share * returned,
                        remaining: bolt.remaining + bolt.flown,
                        flown: 0.,
                        falloff: bolt.falloff,
                        // The guard's own weapon, so the ledger of the
                        // shooter's trade (a rifle's Wilderness XP) is not
                        // paid to a blade for the shooter's bolt.
                        weapon: guard.weapon,
                        effects: ShotEffects {
                            reflected: true,
                            ..ShotEffects::default()
                        },
                    });
                }
            }
        }
        if hit.is_some() || bolt.remaining <= 0.001 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_only_hits_each_victim_once() {
        let mut world = World::new();
        let attacker = world.spawn_empty().id();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let mut ledger = ContactLedger::default();
        assert!(ledger.accept(attacker, 1, a));
        assert!(!ledger.accept(attacker, 1, a));
        assert!(ledger.accept(attacker, 1, b));
        assert!(ledger.accept(attacker, 2, a));
    }

    #[test]
    fn opening_and_return_cuts_cross_in_front_of_the_actor() {
        for action in [Action::Light, Action::Followup] {
            let spec = action.spec();
            let midpoint = blade_point(
                action,
                &spec,
                spec.contact_start.midpoint(spec.contact_end),
                1.65,
            );
            assert!(midpoint.x.abs() < 0.001 && midpoint.z < -1.6);
        }
    }
}
