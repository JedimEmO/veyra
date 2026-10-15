//! The fight's effects past the muzzle flash and the impact spark: a cut's
//! arc, a guard met, parried or broken, a blast's ring, a status on its
//! bearer, a death's scatter, a dodge's and a landing's dust, and the cap on
//! how many of them live at once.
//!
//! All of it is read from what the sim already raises (`Combatant`'s action
//! and statuses, `CombatImpact`) and drawn through the same [`Pulse`], so the
//! fade steps and the cap are shared. Nothing here decides a rule.

use std::collections::HashMap;

use strata_physics::CharacterController;
use strata_physics::combat::blade_point;
use strata_sim::combat::Action;

use super::field::{DUST, FieldTints, feet, tint_of};
// The parent's private pulse vocabulary, which this module only extends.
#[allow(clippy::wildcard_imports, reason = "the fx module's private items")]
use super::*;

/// The most transient pulses alive at once. A held trigger on a guard, a
/// crowd's deaths and a bike's dust together stay under it; over it the
/// oldest go first. The bike's glow and a status's mark are one each and
/// are not counted.
pub(super) const PULSE_CAP: usize = 192;
/// A lifetime this long marks a pulse kept for as long as something lasts.
const KEPT: f32 = 1.0e6;
/// Seconds a light cut's and a heavy cut's arc last.
const LIGHT_ARC_SECS: f32 = 0.14;
const HEAVY_ARC_SECS: f32 = 0.2;
/// How far inside the blade's tip a cut's band starts, as a share of the
/// reach: a heavy cut's band is the wider.
const LIGHT_BAND: f32 = 0.72;
const HEAVY_BAND: f32 = 0.5;
/// Segments a cut's arc is built from.
const ARC_SEGMENTS: u16 = 16;
/// Where the heavy cut's sweep centres, from the actor's centre, as the
/// weapon's own curve has it (`strata_physics::combat::blade_point`).
const HEAVY_CENTRE: Vec3 = Vec3::new(0.12, 0.35, 0.);
const LIGHT_CENTRE: Vec3 = Vec3::new(0., 0.2, 0.);
/// Seconds one breath of a status glow takes, and how deep it goes.
const BREATH_SECS: f32 = 2.4;
const BREATH_DEPTH: f32 = 0.45;
/// A fall faster than this, metres a second, raises dust on landing.
const HARD_LANDING: f32 = 3.;

/// The glow of a blast: a warm light that stays see-through.
const BLAST_GLOW: LinearRgba = LinearRgba::new(2.4, 0.9, 0.15, 0.15);
/// The glow a death leaves.
const DEATH_GLOW: LinearRgba = LinearRgba::new(2.2, 0.3, 0.1, 0.16);

/// A blast went off at a point, to this radius. The sim raises no message
/// for one, so the audio module tells it from a bolt that carried a blast
/// and is gone when its impact arrives, and the ring reads it from there.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlastWent {
    /// Where the bolt met what it met.
    pub(crate) point: WorldPos,
    /// Metres the blast reaches.
    pub(crate) radius: f32,
}

/// A cut's arc, one mesh a cut action, built once from the weapon's own
/// sweep.
#[derive(Resource, Default)]
struct ArcMeshes(Vec<(Action, Handle<Mesh>)>);

/// The glow that shows `id` on `bearer`, kept for as long as it is borne.
#[derive(Component)]
pub(super) struct StatusMark {
    bearer: Entity,
    id: String,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ArcMeshes>()
        .add_message::<BlastWent>()
        .add_systems(
            Update,
            (
                cut_arcs,
                guard_effects,
                blast_rings,
                deaths,
                status_glows,
                footfalls,
            )
                .chain()
                .run_if(resource_exists::<FxAssets>)
                .in_set(crate::ClientSet::Hud)
                .before(update_pulses),
        )
        // After every spawn of the frame has been applied, so the cap holds
        // for the frame that draws them, not a frame later.
        .add_systems(
            PostUpdate,
            cap_pulses.before(bevy::transform::TransformSystems::Propagate),
        );
}

/// The angle a point of a cut's sweep is at, from the actor's facing.
fn sweep_angle(point: Vec3, heavy: bool) -> f32 {
    if heavy {
        (HEAVY_CENTRE.y - point.y).atan2(-point.z)
    } else {
        point.x.atan2(-point.z)
    }
}

/// A cut's band: the sweep from angle `from` to `to`, a thin soft edge on
/// the inside and the strong one on the outside, brightening along the
/// sweep so its tail trails behind its head.
fn arc_mesh(from: f32, to: f32, heavy: bool, inner: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=ARC_SEGMENTS {
        let t = f32::from(i) / f32::from(ARC_SEGMENTS);
        let (sin, cos) = (from + (to - from) * t).sin_cos();
        let (edge, up) = if heavy {
            (Vec3::new(0., -sin, -cos), [1., 0., 0.])
        } else {
            (Vec3::new(sin, 0., -cos), [0., 1., 0.])
        };
        positions.push((edge * inner).to_array());
        positions.push(edge.to_array());
        normals.extend([up, up]);
        colours.push([1., 1., 1., 0.]);
        colours.push([1., 1., 1., 0.1 + 0.9 * t * t]);
        if i < ARC_SEGMENTS {
            let a = u32::from(i) * 2;
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U32(indices))
}

/// `Update`: a Light, Followup or Heavy that has reached its contact window
/// draws its arc once, a band through the weapon's own reach and sweep in
/// the blade's colour, on a heavy cut wider and longer lived.
fn cut_arcs(
    mut commands: Commands,
    assets: Res<FxAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut arcs: ResMut<ArcMeshes>,
    mut drawn: Local<HashMap<Entity, u64>>,
    actors: Query<(Entity, &Combatant, &WorldPosition)>,
) {
    drawn.retain(|entity, _| actors.contains(*entity));
    for (entity, actor, position) in &actors {
        let action = actor.action;
        if !matches!(action, Action::Light | Action::Followup | Action::Heavy)
            || !actor.weapon.is_melee()
        {
            continue;
        }
        let spec = actor.spec(action);
        if actor.age < spec.contact_start || drawn.get(&entity) == Some(&actor.sequence) {
            continue;
        }
        drawn.insert(entity, actor.sequence);
        let heavy = action == Action::Heavy;
        // The tip of the sweep, as the contact reads it: the baked blade's
        // reach scaled by the class, plus the hilt.
        let tip = 0.45 + 1.2 * actor.reach_factor();
        let angles = [spec.contact_start, spec.contact_end]
            .map(|time| sweep_angle(blade_point(action, &spec, time, 1.), heavy));
        let mesh = if let Some((_, mesh)) = arcs.0.iter().find(|(own, _)| *own == action) {
            mesh.clone()
        } else {
            let band = if heavy { HEAVY_BAND } else { LIGHT_BAND };
            let mesh = meshes.add(arc_mesh(angles[0], angles[1], heavy, band));
            arcs.0.push((action, mesh.clone()));
            mesh
        };
        let facing = Quat::from_rotation_y(actor.facing);
        let centre = if heavy { HEAVY_CENTRE } else { LIGHT_CENTRE };
        let mut arc = Pulse::glow(
            position.0,
            usize::from(actor.team != 0),
            Vec3::splat(tip),
            if heavy {
                HEAVY_ARC_SECS
            } else {
                LIGHT_ARC_SECS
            },
        );
        arc.shape = Shape::Arc;
        arc.mesh = Some(mesh);
        arc.rotation = facing;
        arc.growth = if heavy { 0.3 } else { 0.15 };
        arc.follow = Some((entity, (facing * centre).as_dvec3()));
        pulse(&mut commands, &assets, arc);
    }
}

/// A fan of `count` streaks from `point` about `basis`'s Z, each a little
/// faster than the last.
fn sparks(
    commands: &mut Commands,
    assets: &FxAssets,
    point: WorldPos,
    basis: Quat,
    tint: usize,
    count: u16,
    speed: f32,
) {
    for i in 0..count {
        let angle = f32::from(i) * 2.399_963;
        let direction = basis * Vec3::new(angle.cos(), angle.sin(), 0.7).normalize();
        let mut spark = Pulse::glow(
            point,
            tint,
            Vec3::new(0.06, 0.34, 1.),
            0.2 + f32::from(i % 4) * 0.03,
        );
        spark.velocity = direction * (speed + f32::from(i % 5) * 0.5);
        spark.axis = Some(direction);
        pulse(commands, assets, spark);
    }
}

/// A ring at `point` standing against `normal`, growing to `grow` times its
/// size over `lifetime`.
fn ring_at(
    point: WorldPos,
    normal: Vec3,
    tint: usize,
    size: f32,
    grow: f32,
    lifetime: f32,
) -> Pulse {
    let mut ring = Pulse::glow(point, tint, Vec3::splat(size), lifetime);
    ring.shape = Shape::Ring;
    ring.growth = grow;
    ring.rotation = Quat::from_rotation_arc(Vec3::Z, normal);
    ring
}

/// `Update`: a guard met. A block throws sparks and a small ring where the
/// blow met it, a parry a bright flash, and a broken guard a burst of
/// streaks and a ring on the one whose guard it was.
fn guard_effects(
    mut commands: Commands,
    assets: Res<FxAssets>,
    mut impacts: MessageReader<CombatImpact>,
    positions: Query<&WorldPosition>,
) {
    for impact in impacts.read() {
        if impact.guard == GuardOutcome::Open {
            continue;
        }
        let toward = (impact.from.0 - impact.point.0)
            .as_vec3()
            .normalize_or(Vec3::Y);
        let basis = Quat::from_rotation_arc(Vec3::Z, toward);
        let at = WorldPos(impact.point.0 + toward.as_dvec3() * 0.05);
        let follow = impact
            .victim
            .and_then(|victim| Some((victim, impact.point.0 - positions.get(victim).ok()?.0.0)));
        match impact.guard {
            GuardOutcome::Open => {}
            GuardOutcome::Blocked => {
                sparks(&mut commands, &assets, at, basis, 4, 6, 3.2);
                let mut ring = ring_at(at, toward, 4, 0.18, 2.5, 0.2);
                ring.follow = follow;
                pulse(&mut commands, &assets, ring);
            }
            GuardOutcome::Parried => {
                let mut flash = Pulse::glow(at, 4, Vec3::splat(1.), 0.14);
                flash.rotation = basis;
                flash.growth = 0.6;
                flash.follow = follow;
                pulse(&mut commands, &assets, flash);
                sparks(&mut commands, &assets, at, basis, 4, 5, 4.5);
            }
            GuardOutcome::Broken => {
                sparks(&mut commands, &assets, at, basis, 1, 16, 4.5);
                let mut ring = ring_at(at, toward, 4, 0.3, 3., 0.3);
                ring.follow = follow;
                pulse(&mut commands, &assets, ring);
                let mut flash = Pulse::glow(at, 1, Vec3::splat(0.9), 0.18);
                flash.rotation = basis;
                flash.follow = follow;
                pulse(&mut commands, &assets, flash);
            }
        }
    }
}

/// `Update`: a blast went off. A ring that grows to its radius, standing
/// toward the camera so a low view still reads it, a soft glow and sparks
/// thrown up.
#[allow(
    clippy::too_many_arguments,
    reason = "the blasts, the camera and the pulse assets"
)]
fn blast_rings(
    mut commands: Commands,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blasts: MessageReader<BlastWent>,
    camera: Query<&Transform, With<super::super::combat::CombatCamera>>,
) {
    let facing = camera
        .single()
        .map_or(Quat::from_rotation_arc(Vec3::Z, Vec3::Y), |pose| {
            pose.rotation
        });
    for blast in blasts.read() {
        let tint = tint_of(&mut assets, &mut tints, &mut materials, BLAST_GLOW);
        // The ring mesh's crest is at three quarters of its radius.
        let growth = blast.radius / 0.75 - 1.;
        let mut ring = ring_at(blast.point, Vec3::Z, 4, 1., growth, 0.35);
        ring.rotation = facing;
        pulse(&mut commands, &assets, ring);
        let mut glow = Pulse::glow(blast.point, tint, Vec3::splat(blast.radius * 0.2), 0.2);
        glow.shape = Shape::Shield;
        glow.growth = 0.8;
        pulse(&mut commands, &assets, glow);
        sparks(
            &mut commands,
            &assets,
            blast.point,
            Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
            4,
            12,
            4.,
        );
    }
}

/// `Update`: a death, a scatter of streaks and a glow that fades, where the
/// killing blow landed.
fn deaths(
    mut commands: Commands,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impacts: MessageReader<CombatImpact>,
    positions: Query<&WorldPosition>,
) {
    for impact in impacts.read().filter(|impact| impact.lethal) {
        let at = impact
            .victim
            .and_then(|victim| positions.get(victim).ok())
            .map_or(impact.point, |position| {
                WorldPos(position.0.0 + bevy::math::DVec3::Y * 0.2)
            });
        for i in 0..18_u16 {
            let angle = f32::from(i) * 2.399_963;
            let lift = (f32::from(i % 6) - 1.) * 0.25;
            let direction = Vec3::new(angle.cos(), lift + 0.4, angle.sin()).normalize();
            let tint = if i % 3 == 0 { 4 } else { 3 };
            let mut spark = Pulse::glow(
                at,
                tint,
                Vec3::new(0.06, 0.34, 1.),
                0.4 + f32::from(i % 5) * 0.06,
            );
            spark.velocity = direction * (4.5 + f32::from(i % 7) * 0.8);
            spark.axis = Some(direction);
            pulse(&mut commands, &assets, spark);
        }
        let tint = tint_of(&mut assets, &mut tints, &mut materials, DEATH_GLOW);
        let mut glow = Pulse::glow(at, tint, Vec3::splat(0.45), 0.5);
        glow.shape = Shape::Shield;
        glow.growth = 1.5;
        pulse(&mut commands, &assets, glow);
    }
}

/// The colour a status glows in, by its id in `abilities.ron`, or none for
/// a status the view has no colour for.
fn status_colour(id: &str) -> Option<LinearRgba> {
    let [r, g, b] = match id {
        "focused" => [0.35, 0.85, 1.],
        "fortified" => [1., 0.72, 0.2],
        "reflecting" => [0.75, 0.4, 1.],
        "slowed" => [0.2, 0.9, 0.55],
        "disarmed" => [0.9, 0.3, 0.3],
        "shield_broken" => [1., 0.45, 0.1],
        _ => return None,
    };
    Some(LinearRgba::new(r, g, b, 0.3))
}

/// Whether `actor` bears `id` now.
fn bears(actor: &Combatant, id: &str) -> bool {
    actor.action != Action::Dead
        && actor
            .statuses()
            .iter()
            .any(|(held, left)| held == id && *left > 0.)
}

/// `Update`: a slow breathing glow on whoever bears a status, in the
/// status's tint, from when it is put on to when it runs out, on the
/// player and on hostiles alike.
#[allow(
    clippy::too_many_arguments,
    reason = "the clock, the bearers, the marks and the pulse assets"
)]
fn status_glows(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    actors: Query<(Entity, &Combatant, &WorldPosition)>,
    mut marks: Query<(Entity, &StatusMark, &mut Pulse)>,
) {
    let mut marked = Vec::new();
    for (entity, mark, mut glow) in &mut marks {
        if !actors
            .get(mark.bearer)
            .is_ok_and(|(_, actor, _)| bears(actor, &mark.id))
        {
            commands.entity(entity).despawn();
            continue;
        }
        marked.push((mark.bearer, mark.id.clone()));
        // The breath moves the fade step and the size together, and the
        // age never reaches the lifetime, so the mark is never retired.
        let phase = time.elapsed_secs() / BREATH_SECS * std::f32::consts::TAU
            + f32::from(u8::try_from(mark.bearer.index_u32() % 7).unwrap_or(0));
        let breath = 0.5 + 0.5 * phase.sin();
        glow.age = breath * BREATH_DEPTH;
        glow.size = Vec3::new(0.7, 1.05, 0.7) * (0.94 + 0.12 * breath);
    }
    for (entity, actor, position) in &actors {
        for (id, left) in actor.statuses() {
            if *left <= 0.
                || actor.action == Action::Dead
                || marked
                    .iter()
                    .any(|(bearer, held)| *bearer == entity && held == id)
            {
                continue;
            }
            let Some(colour) = status_colour(id) else {
                continue;
            };
            let tint = tint_of(&mut assets, &mut tints, &mut materials, colour);
            let mut glow = Pulse::glow(position.0, tint, Vec3::new(0.7, 1.05, 0.7), 1.);
            glow.shape = Shape::Shield;
            glow.follow = Some((entity, bevy::math::DVec3::ZERO));
            let mesh = assets.sphere.clone();
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(assets.fades[tint][0].clone()),
                Transform::default(),
                bevy::light::NotShadowCaster,
                glow,
                StatusMark {
                    bearer: entity,
                    id: id.clone(),
                },
            ));
        }
    }
}

/// What a footfall pass remembers of an actor: its action, whether it was
/// grounded and how fast it fell.
#[derive(Clone, Copy)]
struct Stance {
    action: Action,
    grounded: bool,
    fall: f32,
}

/// `Update`: a dodge's start and a hard landing each raise a puff of dust
/// at the feet.
fn footfalls(
    mut commands: Commands,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut before: Local<HashMap<Entity, Stance>>,
    actors: Query<(
        Entity,
        &Combatant,
        &WorldPosition,
        Option<&CharacterController>,
    )>,
) {
    before.retain(|entity, _| actors.contains(*entity));
    let dust = tint_of(&mut assets, &mut tints, &mut materials, DUST);
    for (entity, actor, position, motor) in &actors {
        let Some(motor) = motor else {
            continue;
        };
        let now = Stance {
            action: actor.action,
            grounded: motor.grounded,
            fall: if motor.grounded {
                0.
            } else {
                motor.vertical_velocity.min(0.)
            },
        };
        let was = before.insert(entity, now);
        let Some(was) = was else {
            continue;
        };
        let dodged = was.action != Action::Dodge && now.action == Action::Dodge;
        let landed = !was.grounded && now.grounded && -was.fall > HARD_LANDING;
        for (raised, size, count) in [(dodged, 0.28, 3_u16), (landed, 0.4, 4)] {
            if !raised {
                continue;
            }
            for i in 0..count {
                let angle = f32::from(i) * 2.1;
                let mut at = feet(position, Some(motor));
                at.0.y += 0.1;
                let mut puff = Pulse::glow(at, dust, Vec3::splat(size), 0.55);
                puff.shape = Shape::Shield;
                puff.growth = 2.4;
                puff.velocity = Vec3::new(angle.cos(), 0.25, angle.sin()) * 0.9;
                pulse(&mut commands, &assets, puff);
            }
        }
    }
}

/// `Update`: over [`PULSE_CAP`] live transient pulses the oldest are dropped
/// first, so no scene can swell the count past it.
fn cap_pulses(mut commands: Commands, pulses: Query<(Entity, &Pulse), Without<StatusMark>>) {
    let mut live: Vec<(Entity, f32)> = pulses
        .iter()
        .filter(|(_, effect)| effect.lifetime < KEPT)
        .map(|(entity, effect)| (entity, effect.age))
        .collect();
    if live.len() <= PULSE_CAP {
        return;
    }
    // Oldest first; a tie goes to the earlier entity, which was spawned first.
    live.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    for (entity, _) in live.iter().take(live.len() - PULSE_CAP) {
        commands.entity(*entity).despawn();
    }
}

#[cfg(test)]
mod tests;
