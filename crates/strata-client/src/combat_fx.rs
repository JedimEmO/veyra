//! World-space energy fire and contact reactions, with bounded light and trail costs.

use std::collections::VecDeque;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use strata_core::WorldPos;
use strata_physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata_sim::combat::{BlasterBolt, CombatImpact, Combatant, GuardOutcome, ShotFired};
use strata_sim::{RenderOrigin, WorldPosition};

mod field;
mod fight;
pub use field::DustPuff;
pub(crate) use fight::BlastWent;

const BOLT_LIGHT_CAP: usize = 4;
const MUZZLE_LIGHT_CAP: usize = 2;
const FADE_STEPS: usize = 8;
const TRAIL_LIFETIME: f32 = 0.10;
const TRAIL_INTERVAL: f32 = 0.025;

#[derive(Resource)]
struct FxAssets {
    sphere: Handle<Mesh>,
    disc: Handle<Mesh>,
    ring: Handle<Mesh>,
    cores: [Handle<StandardMaterial>; 2],
    fades: Vec<[Handle<StandardMaterial>; FADE_STEPS]>,
}

#[derive(Resource, Default)]
struct LightBudget {
    bolts: VecDeque<Entity>,
    muzzles: VecDeque<Entity>,
}

#[derive(Component)]
struct BoltView {
    bolt: Entity,
    /// Where the sim launched it, to measure the bend-in distance.
    start: WorldPos,
    /// Barrel minus gameplay muzzle at launch, faded out over the first metres.
    offset: Vec3,
    previous: WorldPos,
    trail_wait: f32,
    tint: usize,
}

#[derive(Component)]
struct HasBoltView;

/// The art adapter reads this brief local response without changing the motor.
/// A hit kicks a damped spring whose offset the spine chain bends by: a
/// pitch (lean back from a frontal hit) and a roll (away from a side hit),
/// overshooting and settling rather than snapping back.
#[derive(Component, Default)]
pub(crate) struct HitReaction {
    pub(crate) remaining: f32,
    /// Spring position: pitch and roll, radians.
    pub(crate) offset: Vec2,
    /// Spring velocity, radians per second.
    velocity: Vec2,
    shield: f32,
}

impl HitReaction {
    /// Stiffness and damping: about three cycles a second, half critical,
    /// so a shot reads as a jolt and a blade as a stagger that settles.
    const STIFFNESS: f32 = 400.;
    const DAMPING: f32 = 20.;

    /// Kick the spring with a world-space incoming direction and strength.
    pub(crate) fn kick(&mut self, incoming: Vec3, facing: f32, strength: f32) {
        let forward = strata_sim::PlayerIntent::forward(facing);
        let right = strata_sim::PlayerIntent::right(facing);
        // From the front the body pitches back; from the left the top of the
        // spine moves right, which is a negative roll about the forward axis.
        self.velocity += Vec2::new(-forward.dot(incoming), -right.dot(incoming)) * strength;
    }

    /// Integrate one frame; semi-implicit Euler is stable at these rates.
    pub(crate) fn settle(&mut self, dt: f32) {
        let dt = dt.min(0.05);
        self.velocity += (-Self::STIFFNESS * self.offset - Self::DAMPING * self.velocity) * dt;
        self.offset += self.velocity * dt;
        self.offset = self.offset.clamp(Vec2::splat(-0.6), Vec2::splat(0.6));
    }
}

/// Where the view last saw an actor's barrel relative to the sim's gameplay
/// muzzle, render space, taken as its shot was fired.
#[derive(Component, Default)]
pub(crate) struct MuzzleOffset(Option<Vec3>);

#[derive(Clone, Copy)]
enum Shape {
    Glow,
    Ring,
    Shield,
    // A swept band that carries its own mesh, for a cut.
    Arc,
}

#[derive(Component)]
struct Pulse {
    point: WorldPos,
    velocity: Vec3,
    rotation: Quat,
    size: Vec3,
    growth: f32,
    age: f32,
    lifetime: f32,
    tint: usize,
    shape: Shape,
    // Moving sparks and trails face the camera while keeping their long axis.
    axis: Option<Vec3>,
    follow: Option<(Entity, bevy::math::DVec3)>,
    frames: Option<u8>,
    // The mesh of a pulse whose shape is not one of the shared three.
    mesh: Option<Handle<Mesh>>,
}

impl Pulse {
    fn glow(point: WorldPos, tint: usize, size: Vec3, lifetime: f32) -> Self {
        Self {
            point,
            tint,
            size,
            lifetime,
            velocity: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            growth: 0.,
            age: 0.,
            shape: Shape::Glow,
            axis: None,
            follow: None,
            frames: None,
            mesh: None,
        }
    }
}

#[derive(Component)]
struct MuzzleLight {
    point: WorldPos,
    frames: u8,
}

pub(super) fn install(app: &mut App) {
    field::install(app);
    fight::install(app);
    app.init_resource::<LightBudget>()
        .add_systems(Startup, load)
        .add_systems(
            Update,
            (
                track_reactions,
                contacts,
                draw_bolts,
                update_pulses,
                update_lights,
            )
                .chain()
                .in_set(crate::ClientSet::Hud),
        );
}

fn load(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let colours = [
        LinearRgba::rgb(0.08, 1.8, 3.2),
        LinearRgba::rgb(3.2, 0.65, 0.06),
        LinearRgba::rgb(0.12, 1.5, 3.0),
        LinearRgba::rgb(2.4, 0.22, 0.08),
        LinearRgba::rgb(2.8, 1.0, 0.18),
    ];
    let cores = std::array::from_fn(|i| {
        materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: colours[i] * 5. + LinearRgba::rgb(2., 2., 2.),
            emissive_exposure_weight: 0.,
            ..default()
        })
    });
    let fades = colours
        .into_iter()
        .map(|colour| {
            std::array::from_fn(|i| {
                let mut colour = colour;
                #[allow(clippy::cast_precision_loss, reason = "eight bounded fade steps")]
                {
                    colour.alpha = (1. - i as f32 / FADE_STEPS as f32).powi(2);
                }
                materials.add(StandardMaterial {
                    base_color: Color::LinearRgba(colour),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    fog_enabled: false,
                    ..default()
                })
            })
        })
        .collect();
    commands.insert_resource(FxAssets {
        sphere: meshes.add(Sphere::new(1.).mesh().uv(12, 8)),
        disc: meshes.add(soft_disc(false)),
        ring: meshes.add(soft_disc(true)),
        cores,
        fades,
    });
}

// Vertex alpha supplies soft edges without texture loads or another shader pipeline.
fn soft_disc(ring: bool) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colours = Vec::new();
    let mut indices = Vec::new();
    for band in 0..=8_u16 {
        let radius = f32::from(band) / 8.;
        let alpha = if ring {
            (1. - ((radius - 0.75) / 0.25).abs()).max(0.)
        } else {
            (1. - radius * radius).powi(3)
        };
        for slice in 0..=24_u16 {
            let angle = f32::from(slice) / 24. * std::f32::consts::TAU;
            positions.push([radius * angle.cos(), radius * angle.sin(), 0.]);
            normals.push([0., 0., 1.]);
            colours.push([1., 1., 1., alpha]);
            if band < 8 && slice < 24 {
                let a = u32::from(band) * 25 + u32::from(slice);
                indices.extend_from_slice(&[a, a + 25, a + 1, a + 1, a + 25, a + 26]);
            }
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

fn track_reactions(
    mut commands: Commands,
    time: Res<Time>,
    added: Query<(Entity, &Combatant), Without<HitReaction>>,
    mut reactions: Query<&mut HitReaction>,
) {
    for (entity, actor) in &added {
        commands.entity(entity).insert((
            HitReaction {
                remaining: 0.,
                offset: Vec2::ZERO,
                velocity: Vec2::ZERO,
                shield: actor.shield,
            },
            MuzzleOffset::default(),
        ));
    }
    for mut reaction in &mut reactions {
        reaction.remaining = (reaction.remaining - time.delta_secs()).max(0.);
        reaction.settle(time.delta_secs());
    }
}

fn pulse(commands: &mut Commands, assets: &FxAssets, effect: Pulse) {
    let mesh = effect.mesh.clone().unwrap_or_else(|| match effect.shape {
        Shape::Glow | Shape::Arc => assets.disc.clone(),
        Shape::Ring => assets.ring.clone(),
        Shape::Shield => assets.sphere.clone(),
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(assets.fades[effect.tint][0].clone()),
        Transform::default(),
        NotShadowCaster,
        effect,
    ));
}

fn light(tint: usize, intensity: f32, range: f32) -> PointLight {
    PointLight {
        color: if tint == 1 {
            Color::srgb(1., 0.25, 0.025)
        } else {
            Color::srgb(0.06, 0.65, 1.)
        },
        intensity,
        range,
        radius: 0.12,
        shadow_maps_enabled: false,
        ..default()
    }
}

fn reserve_light(
    commands: &mut Commands,
    queue: &mut VecDeque<Entity>,
    entity: Entity,
    cap: usize,
) {
    queue.push_back(entity);
    while queue.len() > cap {
        if let Some(oldest) = queue.pop_front() {
            commands.entity(oldest).remove::<PointLight>();
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "launch and collision messages share bounded presentation assets"
)]
fn contacts(
    mut commands: Commands,
    mut shots: MessageReader<ShotFired>,
    mut impacts: MessageReader<CombatImpact>,
    mut actors: Query<(&Combatant, &WorldPosition, &mut HitReaction)>,
    mut offsets: Query<&mut MuzzleOffset>,
    muzzles: Query<(&crate::combat_art::Muzzle, &GlobalTransform)>,
    lights: Query<(), With<PointLight>>,
    mut budget: ResMut<LightBudget>,
    assets: Res<FxAssets>,
    spatial: SpatialQuery,
    origin: Res<RenderOrigin>,
) {
    budget.muzzles.retain(|entity| lights.contains(*entity));
    for shot in shots.read() {
        let (tint, shown) = actors
            .get(shot.attacker)
            .map_or((0, None), |(actor, _, _)| {
                (
                    usize::from(actor.team != 0),
                    crate::combat_art::shown_weapon(actor),
                )
            });
        // The flash and the bolt start at the barrel the view is drawing;
        // the sim's muzzle is a fixed offset from the capsule.
        let barrel = muzzles.iter().find_map(|(muzzle, transform)| {
            (muzzle.actor == shot.attacker && shown == Some(muzzle.kind))
                .then(|| transform.translation())
        });
        let from = barrel.map_or(shot.from, |barrel| origin.to_world(barrel));
        if let Ok(mut offset) = offsets.get_mut(shot.attacker) {
            offset.0 = barrel.map(|barrel| barrel - origin.to_render(shot.from));
        }
        let mut flash = Pulse::glow(from, tint, Vec3::new(0.25, 0.6, 1.), 0.05);
        flash.axis = Some(shot.direction);
        flash.frames = Some(3);
        pulse(&mut commands, &assets, flash);
        let entity = commands
            .spawn((
                light(tint, 7_000., 3.),
                MuzzleLight {
                    point: from,
                    frames: 3,
                },
                Transform::from_translation(origin.to_render(from)),
            ))
            .id();
        reserve_light(&mut commands, &mut budget.muzzles, entity, MUZZLE_LIGHT_CAP);
    }
    for impact in impacts.read() {
        let incoming = (impact.point.0 - impact.from.0)
            .as_vec3()
            .normalize_or(Vec3::NEG_Z);
        // Presentation alone recovers the surface normal; no protocol or sim FX data.
        let normal = spatial
            .cast_ray(
                origin.to_render(impact.point) - incoming * 0.3,
                Dir3::new_unchecked(incoming),
                0.7,
                true,
                &SpatialQueryFilter::from_excluded_entities([impact.attacker]),
            )
            .map_or(-incoming, |hit| hit.normal);
        let mut tint = 4;
        let mut follow = None;
        if let Some(victim) = impact.victim
            && let Ok((actor, position, mut reaction)) = actors.get_mut(victim)
        {
            tint = if reaction.shield > actor.shield { 2 } else { 3 };
            reaction.shield = actor.shield;
            // A parry took nothing, so the body neither flinches nor flashes;
            // the spark at the contact point still shows the blades met.
            let parried = impact.guard == GuardOutcome::Parried;
            if !parried {
                reaction.remaining = 0.14;
            }
            let strength = match (impact.melee, tint) {
                (true, _) => 5.5,
                (false, 2) => 1.6,
                (false, _) => 2.8,
            };
            if !parried {
                reaction.kick(incoming, actor.facing, strength);
            }
            follow = Some((victim, impact.point.0 - position.0.0));
            if tint == 2 {
                let mut shield = Pulse::glow(position.0, 2, Vec3::new(0.55, 1.05, 0.55), 0.22);
                shield.shape = Shape::Shield;
                shield.follow = Some((victim, bevy::math::DVec3::ZERO));
                // A late palette entry gives the volume a translucent tint.
                shield.age = 0.13;
                pulse(&mut commands, &assets, shield);
                let mut ripple = Pulse::glow(impact.point, 2, Vec3::splat(0.28), 0.24);
                ripple.shape = Shape::Ring;
                ripple.growth = 3.;
                ripple.rotation = Quat::from_rotation_arc(Vec3::Z, normal);
                ripple.follow = follow;
                pulse(&mut commands, &assets, ripple);
            }
        }
        let radius = match tint {
            2 => 0.65,
            3 => 0.32,
            _ => 0.46,
        };
        let point = WorldPos(impact.point.0 + normal.as_dvec3() * 0.025);
        let mut flash = Pulse::glow(point, tint, Vec3::splat(radius), 0.12);
        flash.rotation = Quat::from_rotation_arc(Vec3::Z, normal);
        flash.follow = follow;
        flash.growth = 0.5;
        pulse(&mut commands, &assets, flash);
        let basis = Quat::from_rotation_arc(Vec3::Z, normal);
        for i in 0..6_u16 {
            let angle = f32::from(i) * 2.399_963;
            let direction = basis * Vec3::new(angle.cos(), angle.sin(), 0.8).normalize();
            let mut spark = Pulse::glow(
                point,
                tint,
                Vec3::new(0.035, 0.18, 1.),
                0.16 + f32::from(i) * 0.012,
            );
            spark.velocity = direction * (2.5 + f32::from(i) * 0.45);
            spark.axis = Some(direction);
            pulse(&mut commands, &assets, spark);
        }
    }
    for (actor, _, mut reaction) in &mut actors {
        reaction.shield = actor.shield;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "bolt presentation samples sim positions and manages a separate light budget"
)]
fn draw_bolts(
    mut commands: Commands,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    assets: Res<FxAssets>,
    mut budget: ResMut<LightBudget>,
    lights: Query<(), With<PointLight>>,
    added: Query<(Entity, &BlasterBolt), Without<HasBoltView>>,
    bolts: Query<&BlasterBolt>,
    mut offsets: Query<&mut MuzzleOffset>,
    mut views: Query<(Entity, &mut BoltView, &mut Transform)>,
) {
    budget.bolts.retain(|entity| lights.contains(*entity));
    for (entity, bolt) in &added {
        let tint = usize::from(bolt.team != 0);
        let offset = offsets
            .get_mut(bolt.owner)
            .ok()
            .and_then(|mut offset| offset.0.take())
            .unwrap_or(Vec3::ZERO);
        let point = WorldPos(
            bolt.previous
                .0
                .lerp(bolt.position.0, fixed.overstep_fraction_f64()),
        );
        let view = commands
            .spawn((
                BoltView {
                    bolt: entity,
                    start: bolt.previous,
                    offset,
                    previous: WorldPos(bolt.previous.0 + offset.as_dvec3()),
                    trail_wait: 0.,
                    tint,
                },
                light(tint, 3_000., 2.5),
                Transform::from_translation(origin.to_render(point) + offset)
                    .looking_to(bolt.velocity, Vec3::Y),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.cores[tint].clone()),
                    Transform::from_xyz(0., 0., 0.38).with_scale(Vec3::new(0.045, 0.045, 0.42)),
                    NotShadowCaster,
                ));
                for rotation in [
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                        * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ] {
                    parent.spawn((
                        Mesh3d(assets.disc.clone()),
                        MeshMaterial3d(assets.fades[tint][1].clone()),
                        Transform::from_xyz(0., 0., 0.4)
                            .with_rotation(rotation)
                            .with_scale(Vec3::new(0.24, 0.82, 1.)),
                        NotShadowCaster,
                    ));
                }
            })
            .id();
        commands.entity(entity).insert(HasBoltView);
        reserve_light(&mut commands, &mut budget.bolts, view, BOLT_LIGHT_CAP);
    }
    for (entity, mut view, mut transform) in &mut views {
        let Ok(bolt) = bolts.get(view.bolt) else {
            commands.entity(entity).despawn();
            continue;
        };
        let along = WorldPos(
            bolt.previous
                .0
                .lerp(bolt.position.0, fixed.overstep_fraction_f64()),
        );
        // Bend from the barrel onto the authoritative path over the first
        // metres, so the bolt leaves the gun and still lands where the sim says.
        #[allow(clippy::cast_possible_truncation, reason = "a few metres of travel")]
        let bend = bend_in((along.0 - view.start.0).length() as f32);
        let point = WorldPos(along.0 + (view.offset * bend).as_dvec3());
        transform.translation = origin.to_render(point);
        transform.look_to(bolt.velocity - view.offset * bend_slope(bend), Vec3::Y);
        view.trail_wait -= time.delta_secs();
        if view.trail_wait <= 0. {
            let delta = (point.0 - view.previous.0).as_vec3();
            if delta.length_squared() > 0.0001 {
                let mut trail = Pulse::glow(
                    WorldPos(point.0.midpoint(view.previous.0)),
                    view.tint,
                    Vec3::new(0.11, delta.length() * 0.65, 1.),
                    TRAIL_LIFETIME,
                );
                trail.axis = Some(delta.normalize());
                pulse(&mut commands, &assets, trail);
            }
            view.previous = point;
            view.trail_wait = TRAIL_INTERVAL;
        }
    }
}

/// How much of the launch offset remains after `travelled` metres: all of it
/// at the barrel, none past the bend-in distance, eased in between.
fn bend_in(travelled: f32) -> f32 {
    let u = (travelled / BEND_IN_DISTANCE).clamp(0., 1.);
    (1. - u) * (1. - u)
}

/// A velocity-sized correction so the streak points along the bent path
/// rather than the straight one while the offset is still fading.
fn bend_slope(bend: f32) -> f32 {
    bend.sqrt() * 2. * BOLT_BEND_SPEED
}

/// Metres over which a bolt's start converges onto the gameplay path.
const BEND_IN_DISTANCE: f32 = 6.;
/// The straight path is followed at bolt speed; the lateral drift fades at
/// this fraction of it, which keeps the streak's look-to tangent sane.
const BOLT_BEND_SPEED: f32 = 4.;

fn streak_rotation(axis: Vec3, view: Vec3) -> Quat {
    let side = axis.cross(view).normalize_or(axis.any_orthonormal_vector());
    Quat::from_mat3(&Mat3::from_cols(side, axis, side.cross(axis)))
}

fn update_pulses(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<RenderOrigin>,
    assets: Res<FxAssets>,
    camera: Query<&Transform, (With<super::combat::CombatCamera>, Without<Pulse>)>,
    actors: Query<&WorldPosition>,
    mut pulses: Query<(
        Entity,
        &mut Pulse,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let camera = camera.single().map_or(Vec3::ZERO, |pose| pose.translation);
    for (entity, mut effect, mut pose, mut material) in &mut pulses {
        if effect.age >= effect.lifetime || effect.frames == Some(0) {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some((actor, offset)) = effect.follow {
            let Ok(position) = actors.get(actor) else {
                commands.entity(entity).despawn();
                continue;
            };
            effect.point = WorldPos(position.0.0 + offset);
        }
        let fraction = effect
            .frames
            .map_or(effect.age / effect.lifetime, |frames| {
                1. - f32::from(frames) / 3.
            });
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "clamped index into eight fade materials"
        )]
        let fade = ((fraction * FADE_STEPS as f32) as usize).min(FADE_STEPS - 1);
        material.0 = assets.fades[effect.tint][fade].clone();
        pose.translation = origin.to_render(effect.point);
        pose.rotation = effect.axis.map_or(effect.rotation, |axis| {
            streak_rotation(axis, camera - pose.translation)
        });
        pose.scale = effect.size * (1. + effect.growth * fraction);
        let travel = effect.velocity.as_dvec3() * f64::from(time.delta_secs());
        effect.point.0 += travel;
        effect.age += time.delta_secs();
        if let Some(frames) = &mut effect.frames {
            *frames -= 1;
        }
    }
}

fn update_lights(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    mut lights: Query<(Entity, &mut MuzzleLight, &mut Transform)>,
) {
    for (entity, mut flash, mut pose) in &mut lights {
        if flash.frames == 0 {
            commands.entity(entity).despawn();
        } else {
            pose.translation = origin.to_render(flash.point);
            flash.frames -= 1;
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a missing scenario resource or component fails the test"
)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Time>();
        world.init_resource::<Time<Fixed>>();
        world.init_resource::<RenderOrigin>();
        world.init_resource::<LightBudget>();
        world.run_system_once(load).unwrap();
        world
    }

    #[test]
    fn oldest_bolt_lights_drop_at_the_cap_and_views_expire_with_the_sim() {
        let mut world = world();
        let owner = world.spawn_empty().id();
        let bolts: Vec<_> = (0..7)
            .map(|_| {
                world
                    .spawn(BlasterBolt {
                        owner,
                        team: 0,
                        previous: WorldPos::new(0., 0., 0.),
                        position: WorldPos::new(0., 0., -1.),
                        velocity: Vec3::NEG_Z * 38.,
                        damage: 24.,
                        posture: 10.,
                        remaining: 100.,
                        flown: 0.,
                        falloff: strata_sim::combat::Weapon::Carbine.spec().falloff,
                        weapon: strata_sim::combat::Weapon::Carbine,
                        effects: strata_sim::ability::ShotEffects::default(),
                    })
                    .id()
            })
            .collect();
        world.run_system_once(draw_bolts).unwrap();
        for (_, transform) in world.query::<(&BoltView, &Transform)>().iter(&world) {
            assert!(
                transform.translation.length() < 0.001,
                "new views use the same interpolated phase"
            );
        }
        let lit: Vec<_> = world
            .query::<(&BoltView, &PointLight)>()
            .iter(&world)
            .map(|(view, _)| view.bolt)
            .collect();
        assert_eq!(lit.len(), BOLT_LIGHT_CAP);
        for bolt in &bolts[..3] {
            assert!(!lit.contains(bolt), "oldest light removed");
        }
        // A missed bolt still has geometry when its expensive light is retired.
        assert_eq!(world.query::<&BoltView>().iter(&world).count(), 7);
        for bolt in bolts {
            world.despawn(bolt);
        }
        world.run_system_once(draw_bolts).unwrap();
        assert_eq!(world.query::<&BoltView>().iter(&world).count(), 0);
        assert_eq!(world.query::<&PointLight>().iter(&world).count(), 0);
        assert_eq!(world.query::<&Mesh3d>().iter(&world).count(), 0);
    }

    #[test]
    fn muzzle_lights_last_three_frames_and_discard_oldest_over_budget() {
        let mut world = world();
        world
            .run_system_once(|mut commands: Commands, mut budget: ResMut<LightBudget>| {
                for _ in 0..5 {
                    let entity = commands
                        .spawn((
                            light(1, 7_000., 3.),
                            MuzzleLight {
                                point: WorldPos::new(4., 2., 1.),
                                frames: 3,
                            },
                            Transform::default(),
                        ))
                        .id();
                    reserve_light(&mut commands, &mut budget.muzzles, entity, MUZZLE_LIGHT_CAP);
                }
            })
            .unwrap();
        for _ in 0..3 {
            world.run_system_once(update_lights).unwrap();
            assert_eq!(
                world.query::<&PointLight>().iter(&world).count(),
                MUZZLE_LIGHT_CAP
            );
        }
        world.run_system_once(update_lights).unwrap();
        assert_eq!(world.query::<&MuzzleLight>().iter(&world).count(), 0);
    }

    #[test]
    fn trails_rebase_from_world_positions_and_fade_after_bolt_despawn() {
        let mut world = world();
        world
            .run_system_once(|mut commands: Commands, assets: Res<FxAssets>| {
                let mut trail = Pulse::glow(
                    WorldPos::new(20_000., 3., 0.),
                    0,
                    Vec3::new(0.1, 0.5, 1.),
                    0.10,
                );
                trail.axis = Some(Vec3::NEG_Z);
                pulse(&mut commands, &assets, trail);
            })
            .unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(25));
        world.run_system_once(update_pulses).unwrap();
        let before = world
            .query::<(&Pulse, &Transform)>()
            .single(&world)
            .unwrap()
            .1
            .translation;
        world.resource_mut::<RenderOrigin>().0 = WorldPos::new(20_000., 0., 0.);
        world.run_system_once(update_pulses).unwrap();
        let after = world
            .query::<(&Pulse, &Transform)>()
            .single(&world)
            .unwrap()
            .1
            .translation;
        assert!((before.x - after.x - 20_000.).abs() < 0.01);
        assert!(after.is_finite());
        for _ in 0..4 {
            world.run_system_once(update_pulses).unwrap();
        }
        assert_eq!(world.query::<&Pulse>().iter(&world).count(), 0);
    }
    #[test]
    fn confirmed_shield_and_flesh_hits_drive_distinct_effects_and_flinch() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            strata_physics::PhysicsPlugin,
        ));
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1. / 60.),
        ));
        install(&mut app);
        app.finish();
        app.cleanup();
        let attacker = app
            .world_mut()
            .spawn((Combatant::default(), WorldPosition::new(0., 0., 0.)))
            .id();
        let victim = app
            .world_mut()
            .spawn((
                Combatant::on_team(1, 100.),
                WorldPosition::new(0., 0., -20.),
            ))
            .id();
        app.update();
        for expected_tint in [2, 2, 3] {
            app.world_mut()
                .get_mut::<Combatant>(victim)
                .unwrap()
                .damage(24., 5.);
            app.world_mut().write_message(CombatImpact {
                attacker,
                victim: Some(victim),
                from: WorldPos::new(0., 0., -19.),
                point: WorldPos::new(0., 0., -19.7),
                melee: false,
                dealt: 24.,
                lethal: false,
                guard: strata_sim::combat::GuardOutcome::Open,
                shot: None,
            });
            app.update();
            assert!(app.world().get::<HitReaction>(victim).unwrap().remaining > 0.1);
            assert!(
                app.world_mut()
                    .query::<&Pulse>()
                    .iter(app.world())
                    .any(|effect| {
                        effect.tint == expected_tint
                            && effect.follow.is_some()
                            && matches!(effect.shape, Shape::Glow)
                    })
            );
        }
        assert!(
            app.world_mut()
                .query::<&Pulse>()
                .iter(app.world())
                .any(|effect| {
                    matches!(effect.shape, Shape::Ring)
                        && effect.follow.is_some_and(|(actor, _)| actor == victim)
                })
        );
    }

    #[test]
    fn a_frontal_hit_leans_back_overshoots_and_settles() {
        let mut reaction = HitReaction::default();
        // Facing -Z; the shot comes from the front, travelling +Z.
        reaction.kick(Vec3::Z, 0., 2.8);
        let mut peak = 0_f32;
        let mut crossed = false;
        for _ in 0..120 {
            reaction.settle(1. / 60.);
            peak = peak.max(reaction.offset.x);
            if reaction.offset.x < -0.005 {
                crossed = true;
            }
        }
        assert!(peak > 0.05 && peak < 0.3, "a readable lean back: {peak}");
        assert!(crossed, "half-critical damping overshoots once");
        assert!(
            reaction.offset.length() < 0.01,
            "settled within two seconds"
        );
        assert!(
            reaction.offset.y.abs() < 0.001,
            "no roll from a frontal hit"
        );
        let mut side = HitReaction::default();
        side.kick(Vec3::X, 0., 2.8);
        side.settle(1. / 60.);
        assert!(
            side.offset.y < 0.,
            "a hit from the left rolls the spine right"
        );
    }
}
