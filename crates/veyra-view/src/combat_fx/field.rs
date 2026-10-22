//! The field's motion: a ping running out over the ground, a sampling member
//! kneeling with sparks at the ground, and the bike's dust and glow.
//!
//! All of it is read from components the sim already keeps (`Pinging`,
//! `Sampling`, `Riding`) and drawn through the same [`Pulse`] the fight uses,
//! so the pulse cap and the fade steps are shared. A flat ring needs no shape
//! of its own: the ring mesh with a rotation that lays it on the ground is
//! not turned to the camera, because only a pulse with an `axis` is.

use std::collections::HashMap;

use strata_physics::CharacterController;
use veyra_sim::combat::Combatant;
use veyra_sim::deposits::{Pinging, Sampling, skill_of};
use veyra_sim::items::Pack;
use veyra_sim::progression::CharacterSheet;
use veyra_sim::ride::Riding;
use veyra_sim::{PlayerIntent, PreviousWorldPosition, Registries, WorldSourceRes};

use crate::combat_ik::{Kneel, Legs};

// The parent's private pulse vocabulary, which this module only extends.
#[allow(clippy::wildcard_imports, reason = "the fx module's private items")]
use super::*;

/// A ping's band is a unit-radius cylinder this tall (a share of its
/// radius, since a pulse scales evenly), a quarter of it below the feet so a
/// slope cannot bury it. Standing up, it reads from the play camera's low
/// eye where a ring lying on the ground is edge on.
const BAND_HEIGHT: f32 = 0.02;
/// The ping band's colour gain over the class's colour: enough to glow, not
/// enough to wash the class's hue out to white.
const BAND_GAIN: f32 = 1.4;
/// Sides of the ping band's cylinder.
const BAND_SIDES: u16 = 48;
/// Seconds between the sparks of a sampling cycle's beat.
const SAMPLE_BEAT: f32 = 1. / 3.;
/// Metres in front of the member's feet the sparks fly from.
const SAMPLE_REACH: f32 = 0.8;
/// Dust puffs a second at the bike's top speed over ground, and the share of
/// the top speed under which a bike counts as standing and throws none.
const DUST_RATE: f32 = 14.;
const STANDING_SHARE: f32 = 0.08;
/// Most puffs one bike throws in a frame, so a stalled frame is not a burst.
const DUST_PER_FRAME: u32 = 3;
/// How fast the kneel eases in and out, per second, and how far it lowers
/// the body at its deepest, metres.
const KNEEL_RATE: f32 = 3.5;
const KNEEL_DROP: f32 = 0.42;
/// The thigh and shin folds of the forward leg (foot flat, knee bent) and of
/// the trailing leg (knee to the ground), radians.
const FRONT_THIGH: f32 = 1.25;
const FRONT_SHIN: f32 = 1.3;
const BACK_THIGH: f32 = 0.1;
const BACK_SHIN: f32 = 1.55;

/// Fade materials for the colours the field asks for beyond the fight's five,
/// by the colour that made them, so a class's ring is built once.
#[derive(Resource, Default)]
pub(super) struct FieldTints(Vec<([u8; 4], usize)>, Option<Handle<Mesh>>);

/// A puff of dust at a point, asked for by the game for a prop's moving part
/// (a harvester's stroke), which the client draws as it draws the bike's.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct DustPuff(pub WorldPos);

/// A soft glow kept under a ridden bike.
#[derive(Component)]
struct BikeGlow(Entity);

pub(super) fn install(app: &mut App) {
    app.init_resource::<FieldTints>()
        .add_message::<DustPuff>()
        .add_systems(
            Update,
            (
                ping_rings,
                sample_sparks,
                bike_dust,
                bike_glow,
                prop_puffs,
                ease_kneel,
            )
                .run_if(resource_exists::<FxAssets>)
                .in_set(strata_client::ClientSet::Hud)
                .before(update_pulses),
        )
        .add_systems(
            PostUpdate,
            pose_kneel
                .after(crate::combat_art::PoseOverlays)
                .after(strata_client::ClientSet::Follow)
                .after(crate::combat_ik::place_feet)
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

/// The index of the fade set for `colour`, made and kept on first ask. The
/// alpha is the strength at the start; the steps square it away like the
/// fight's own.
pub(super) fn tint_of(
    assets: &mut FxAssets,
    tints: &mut FieldTints,
    materials: &mut Assets<StandardMaterial>,
    colour: LinearRgba,
) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a key of four clamped channels"
    )]
    let key = [colour.red, colour.green, colour.blue, colour.alpha]
        .map(|channel| (channel.clamp(0., 4.) * 63.) as u8);
    if let Some((_, tint)) = tints.0.iter().find(|(own, _)| *own == key) {
        return *tint;
    }
    let set = std::array::from_fn(|i| {
        let mut step = colour;
        #[allow(clippy::cast_precision_loss, reason = "eight bounded fade steps")]
        {
            step.alpha = colour.alpha * (1. - i as f32 / FADE_STEPS as f32).powi(2);
        }
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(step),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        })
    });
    assets.fades.push(set);
    let tint = assets.fades.len() - 1;
    tints.0.push((key, tint));
    tint
}

/// The dust colour, a warm grey: dust is not light, so it is dim and wide.
pub(super) const DUST: LinearRgba = LinearRgba::new(0.5, 0.42, 0.32, 0.22);

/// The member's feet, from the capsule's centre.
pub(super) fn feet(position: &WorldPosition, controller: Option<&CharacterController>) -> WorldPos {
    let drop = controller.map_or(0.9, |motor| f64::from(motor.half_height()));
    WorldPos::new(position.0.x(), position.0.y() - drop, position.0.z())
}

/// An open cylinder of radius one, fading to nothing at its top.
fn band_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for side in 0..=BAND_SIDES {
        let angle = f32::from(side) / f32::from(BAND_SIDES) * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        positions.push([cos, -0.25 * BAND_HEIGHT, sin]);
        positions.push([cos, 0.75 * BAND_HEIGHT, sin]);
        normals.extend([[cos, 0., sin]; 2]);
        colours.push([1., 1., 1., 1.]);
        colours.push([1., 1., 1., 0.]);
        if side < BAND_SIDES {
            let a = u32::from(side) * 2;
            indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
    }
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// `Update`: a ping begun, a ring on the ground at the member that runs out
/// to the ping's real range over the ping's channel, in the class's colour.
fn ping_rings(
    mut commands: Commands,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    registries: Option<Res<Registries>>,
    started: Query<
        (
            &WorldPosition,
            &Pinging,
            &Pack,
            &CharacterSheet,
            Option<&CharacterController>,
        ),
        Added<Pinging>,
    >,
) {
    let Some(registries) = registries else {
        return;
    };
    for (position, pinging, pack, sheet, motor) in &started {
        let Some(skill) = skill_of(&registries.0, pack, sheet) else {
            continue;
        };
        let [r, g, b, _] = registries
            .0
            .resources
            .classes()
            .iter()
            .find(|class| class.id == pinging.class)
            .map_or([0.4, 0.8, 1., 1.], |class| class.color.0);
        let colour = Color::srgb(r, g, b).to_linear();
        let tint = tint_of(
            &mut assets,
            &mut tints,
            &mut materials,
            LinearRgba::new(
                colour.red * BAND_GAIN,
                colour.green * BAND_GAIN,
                colour.blue * BAND_GAIN,
                1.,
            ),
        );
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a reach of a few hundred metres"
        )]
        let reach = skill.range_m as f32;
        let band = tints
            .1
            .get_or_insert_with(|| meshes.add(band_mesh()))
            .clone();
        let at = feet(position, motor);
        let mut ring = Pulse::glow(at, tint, Vec3::ONE, skill.channel_s.max(0.2));
        ring.shape = Shape::Ring;
        ring.mesh = Some(band);
        ring.growth = reach - 1.;
        pulse(&mut commands, &assets, ring);
    }
}

/// `Update`: while a member samples, a puff and a few sparks at the ground
/// before them every [`SAMPLE_BEAT`].
fn sample_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut waits: Local<HashMap<Entity, f32>>,
    sampling: Query<
        (
            Entity,
            &WorldPosition,
            &Combatant,
            Option<&CharacterController>,
        ),
        With<Sampling>,
    >,
) {
    waits.retain(|entity, _| sampling.contains(*entity));
    let dust = tint_of(&mut assets, &mut tints, &mut materials, DUST);
    for (entity, position, actor, motor) in &sampling {
        let wait = waits.entry(entity).or_insert(0.);
        *wait -= time.delta_secs();
        if *wait > 0. {
            continue;
        }
        *wait = SAMPLE_BEAT;
        let mut at = feet(position, motor);
        let forward = PlayerIntent::forward(actor.facing);
        at.0 += forward.as_dvec3() * f64::from(SAMPLE_REACH);
        at.0.y += 0.05;
        let mut puff = Pulse::glow(at, dust, Vec3::splat(0.18), 0.55);
        puff.shape = Shape::Shield;
        puff.growth = 2.;
        puff.velocity = Vec3::Y * 0.3;
        pulse(&mut commands, &assets, puff);
        for i in 0..4_u16 {
            let angle = f32::from(i) * 1.7 + time.elapsed_secs();
            let direction = Vec3::new(angle.cos() * 0.6, 1., angle.sin() * 0.6).normalize();
            let mut spark =
                Pulse::glow(at, 4, Vec3::new(0.05, 0.24, 1.), 0.25 + f32::from(i) * 0.02);
            spark.velocity = direction * (1.8 + f32::from(i) * 0.4);
            spark.axis = Some(direction);
            pulse(&mut commands, &assets, spark);
        }
    }
}

/// Dust puffs a second for a bike at `share` of its top speed: none standing
/// and none off the ground or over water, rising to [`DUST_RATE`] at the top.
fn dust_per_second(share: f32, over_ground: bool) -> f32 {
    if !over_ground || share < STANDING_SHARE {
        0.
    } else {
        DUST_RATE * share.min(1.)
    }
}

/// `Update`: a ridden bike throws dust behind and under it by its speed.
#[allow(
    clippy::too_many_arguments,
    reason = "the bike, the ground it is over and the pulse assets"
)]
fn bike_dust(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    source: Option<Res<WorldSourceRes>>,
    mut debt: Local<HashMap<Entity, f32>>,
    mut puffs: Local<u32>,
    riders: Query<(
        Entity,
        &Riding,
        &WorldPosition,
        Option<&CharacterController>,
    )>,
) {
    debt.retain(|entity, _| riders.contains(*entity));
    let dust = tint_of(&mut assets, &mut tints, &mut materials, DUST);
    for (entity, riding, position, motor) in &riders {
        let top = riding
            .dials
            .speed
            .road
            .max(riding.dials.speed.ground)
            .max(0.1);
        let share = riding.speed.abs() / top;
        let grounded = motor.is_none_or(|motor| motor.grounded);
        let wet = source.as_ref().is_some_and(|world| {
            let here = veyra_sim::water_here(world.source(), position.0.x(), position.0.z());
            here.surface_m > here.ground_m
        });
        let owes = debt.entry(entity).or_insert(0.);
        *owes += dust_per_second(share, grounded && !wet) * time.delta_secs();
        let forward = PlayerIntent::forward(riding.heading);
        for _ in 0..DUST_PER_FRAME {
            if *owes < 1. {
                break;
            }
            *owes -= 1.;
            *puffs = puffs.wrapping_add(1);
            let side = PlayerIntent::forward(riding.heading + std::f32::consts::FRAC_PI_2);
            let mut at = feet(position, motor);
            at.0 += (-forward * (0.7 + 0.5 * scatter(*puffs, 1).abs())
                + side * 0.45 * scatter(*puffs, 2))
            .as_dvec3();
            at.0.y += f64::from(0.1 + 0.15 * scatter(*puffs, 3).abs());
            let mut puff = Pulse::glow(
                at,
                dust,
                Vec3::splat(0.2 + 0.14 * scatter(*puffs, 4).abs()),
                0.6 + 0.3 * scatter(*puffs, 5).abs(),
            );
            puff.shape = Shape::Shield;
            puff.growth = 3.;
            puff.velocity = -forward * riding.speed.abs() * 0.1
                + side * scatter(*puffs, 6) * 0.8
                + Vec3::Y * 0.5;
            pulse(&mut commands, &assets, puff);
        }
    }
}

/// A repeatable number in `-1..=1` from a count and a stream, so a trail of
/// puffs scatters without a random source (which a still could not repeat).
fn scatter(count: u32, stream: u32) -> f32 {
    let mut x = count.wrapping_mul(0x9E37_79B1) ^ stream.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    #[allow(clippy::cast_precision_loss, reason = "sixteen bits of a hash")]
    let unit = (x & 0xFFFF) as f32 / 65_535.;
    unit * 2. - 1.
}

/// `Update`: a soft glow under each bike while it is ridden, made when the
/// ride begins and gone when it ends.
#[allow(
    clippy::too_many_arguments,
    reason = "the rider, the interpolation clock, the glow and the pulse assets"
)]
fn bike_glow(
    mut commands: Commands,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    started: Query<(Entity, &WorldPosition, Option<&CharacterController>), Added<Riding>>,
    fixed: Res<Time<Fixed>>,
    riding: Query<
        (
            &WorldPosition,
            &PreviousWorldPosition,
            Option<&CharacterController>,
        ),
        With<Riding>,
    >,
    mut glows: Query<(Entity, &BikeGlow, &mut Pulse)>,
) {
    for (entity, glow, mut pulse) in &mut glows {
        let Ok((position, previous, motor)) = riding.get(glow.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        // The hull is drawn at the interpolated position, so the glow keeps
        // to that and not to the sim's latest tick, which a fast bike has left
        // behind by most of a metre.
        let drop = motor.map_or(0.9, |motor| f64::from(motor.half_height()));
        let drawn = previous.interpolate(position.0, fixed.overstep_fraction_f64());
        if let Some((_, offset)) = pulse.follow.as_mut() {
            *offset = bevy::math::DVec3::new(
                drawn.0.x - position.0.x(),
                drawn.0.y - position.0.y() + 0.12 - drop,
                drawn.0.z - position.0.z(),
            );
        }
    }
    if started.is_empty() {
        return;
    }
    let tint = tint_of(
        &mut assets,
        &mut tints,
        &mut materials,
        LinearRgba::new(0.1, 0.7, 1.4, 0.3),
    );
    for (entity, position, motor) in &started {
        let drop = motor.map_or(0.9, |motor| f64::from(motor.half_height()));
        let mut glow = Pulse::glow(position.0, tint, Vec3::splat(1.5), f32::MAX);
        glow.rotation = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
        glow.follow = Some((entity, bevy::math::DVec3::new(0., 0.12 - drop, 0.)));
        commands.spawn((
            Mesh3d(assets.disc.clone()),
            MeshMaterial3d(assets.fades[tint][0].clone()),
            Transform::default(),
            NotShadowCaster,
            glow,
            BikeGlow(entity),
        ));
    }
}

/// `Update`: a puff for each [`DustPuff`] the game asked for.
fn prop_puffs(
    mut commands: Commands,
    mut asked: MessageReader<DustPuff>,
    mut assets: ResMut<FxAssets>,
    mut tints: ResMut<FieldTints>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if asked.is_empty() {
        return;
    }
    let dust = tint_of(&mut assets, &mut tints, &mut materials, DUST);
    for DustPuff(at) in asked.read() {
        let mut puff = Pulse::glow(*at, dust, Vec3::splat(0.3), 0.7);
        puff.shape = Shape::Shield;
        puff.growth = 2.6;
        puff.velocity = Vec3::Y * 0.3;
        pulse(&mut commands, &assets, puff);
    }
}

/// `amount` moved toward `target` by `dt` at [`KNEEL_RATE`].
fn eased(amount: f32, target: f32, dt: f32) -> f32 {
    let step = KNEEL_RATE * dt;
    if target > amount {
        (amount + step).min(target)
    } else {
        (amount - step).max(target)
    }
}

/// `Update`: the kneel comes on with `Sampling` and eases out after it.
fn ease_kneel(
    mut commands: Commands,
    time: Res<Time>,
    begun: Query<Entity, (Added<Sampling>, Without<Kneel>)>,
    sampling: Query<(), With<Sampling>>,
    mut kneeling: Query<(Entity, &mut Kneel)>,
) {
    for entity in &begun {
        commands.entity(entity).insert(Kneel(0.));
    }
    for (entity, mut kneel) in &mut kneeling {
        let target = f32::from(u8::from(sampling.contains(entity)));
        kneel.0 = eased(kneel.0, target, time.delta_secs());
        if kneel.0 <= 0. && target == 0. {
            commands.entity(entity).remove::<Kneel>();
        }
    }
}

/// `PostUpdate`, with the other pose overlays: a kneeling member's front leg
/// bent with the foot flat and the trailing knee down, and the body lowered
/// to them. An overlay on the animated pose, as the seat is.
fn pose_kneel(kneeling: Query<&Kneel>, legs: Query<&Legs>, mut bones: Query<&mut Transform>) {
    for legs in &legs {
        let Ok(kneel) = kneeling.get(legs.actor) else {
            continue;
        };
        let eased = kneel.0 * kneel.0 * (3. - 2. * kneel.0);
        let [front, back] = legs.sides;
        for (leg, thigh, shin) in [
            (front, FRONT_THIGH, FRONT_SHIN),
            (back, BACK_THIGH, BACK_SHIN),
        ] {
            if let Ok(mut bone) = bones.get_mut(leg.thigh) {
                bone.rotation = Quat::from_rotation_x(-thigh * eased) * bone.rotation;
            }
            if let Ok(mut bone) = bones.get_mut(leg.shin) {
                bone.rotation = Quat::from_rotation_x(shin * eased) * bone.rotation;
            }
        }
        if let Ok(mut root) = bones.get_mut(legs.root) {
            root.translation.y -= KNEEL_DROP * eased;
        }
    }
}

#[cfg(test)]
mod tests;
