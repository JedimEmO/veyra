//! The shield bearer made readable at range: a tall slab held ahead of its
//! body while its guard is up, leaned out at its side while the guard is
//! down, so the opening a gun waits for is seen and not guessed, and a
//! brighter flash when the guard meets a bolt or a cut.

use bevy::prelude::*;
use strata_sim::combat::{Action, CombatImpact, Combatant, GuardOutcome};
use strata_sim::{PlayerIntent, RenderOrigin, WorldPosition};

use crate::ClientSet;

/// Marks an opponent that bears a slab. The game puts it on the bulwark and
/// on no other kind.
#[derive(Component, Debug, Clone, Copy)]
pub struct Shield;

/// The slab's width, height and thickness in metres: wide enough to cover a
/// torso from the front and tall enough to read at 25 m.
const SLAB_SIZE: Vec3 = Vec3::new(0.9, 1.3, 0.08);
/// How far ahead of the body's centre the raised slab is held, metres.
const HELD_AHEAD: f32 = 0.55;
/// How far the lowered slab hangs below the body's centre, metres.
const LOWERED_DROP: f32 = 0.2;
/// How far out to the actor's right the lowered slab hangs, metres.
const LOWERED_ASIDE: f32 = 0.8;
/// How far ahead of the body's centre the lowered slab hangs, metres, so it
/// does not clip the body's front.
const LOWERED_AHEAD: f32 = 0.1;
/// How far the lowered slab leans out from upright, radians: nearly flat,
/// like a shield let fall to the side, so the change from the raised pose
/// reads at range.
const LOWERED_LEAN: f32 = 1.2;
/// How far above the actor's feet-level position the slab's pose is taken,
/// metres: the body's centre.
const CENTRE_LIFT: f32 = 0.1;
/// How long the slab stays bright after its guard meets a blow, seconds.
const FLASH_SECONDS: f32 = 0.2;

#[derive(Resource)]
struct ShieldAssets {
    mesh: Handle<Mesh>,
    plain: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
}

/// One slab: the actor it belongs to and the flash left on it.
#[derive(Component)]
struct SlabView {
    owner: Entity,
    flash: f32,
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(Startup, load_shield)
        .add_systems(Update, draw_slab.in_set(ClientSet::Hud));
}

fn load_shield(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hostile = Color::srgb(1., 0.65, 0.2);
    commands.insert_resource(ShieldAssets {
        mesh: meshes.add(Cuboid::new(SLAB_SIZE.x, SLAB_SIZE.y, SLAB_SIZE.z)),
        plain: materials.add(StandardMaterial {
            base_color: hostile.darker(0.2),
            emissive: hostile.to_linear() * 0.8,
            perceptual_roughness: 0.5,
            metallic: 0.6,
            ..default()
        }),
        flash: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: Color::srgb(1., 0.9, 0.6).to_linear() * 8.,
            ..default()
        }),
    });
}

/// Where the slab sits and how it turns for an actor at `centre` facing
/// `facing`: upright ahead of the body while the guard is up, flat at its
/// right side, leaned out, while it is down.
fn slab_pose(centre: Vec3, facing: f32, guard_up: bool) -> Transform {
    let yaw = Quat::from_rotation_y(facing);
    let forward = PlayerIntent::forward(facing);
    let right = PlayerIntent::right(facing);
    if guard_up {
        Transform::from_translation(centre + forward * HELD_AHEAD).with_rotation(yaw)
    } else {
        Transform::from_translation(
            centre + right * LOWERED_ASIDE - Vec3::Y * LOWERED_DROP + forward * LOWERED_AHEAD,
        )
        .with_rotation(yaw * Quat::from_rotation_z(-LOWERED_LEAN))
    }
}

#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "one query each for the bearers and the slabs"
)]
fn draw_slab(
    time: Res<Time>,
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    assets: Option<Res<ShieldAssets>>,
    mut impacts: MessageReader<CombatImpact>,
    added: Query<Entity, Added<Shield>>,
    bearers: Query<(&WorldPosition, &Combatant), With<Shield>>,
    mut slabs: Query<(
        Entity,
        &mut SlabView,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
) {
    let Some(assets) = assets else { return };
    for bearer in &added {
        commands.spawn((
            SlabView {
                owner: bearer,
                flash: 0.,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.plain.clone()),
            Transform::default(),
            Visibility::default(),
        ));
    }
    let struck: Vec<Entity> = impacts
        .read()
        .filter(|impact| impact.guard != GuardOutcome::Open)
        .filter_map(|impact| impact.victim)
        .collect();
    for (entity, mut view, mut transform, mut material, mut visibility) in &mut slabs {
        let Ok((position, actor)) = bearers.get(view.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        // A dead bulwark keeps its entity until a reset, and a slab hanging
        // in the air beside the corpse would read as a second bearer.
        let shown = if actor.action == Action::Dead {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != shown {
            *visibility = shown;
        }
        if struck.contains(&view.owner) {
            view.flash = FLASH_SECONDS;
        } else {
            view.flash = (view.flash - time.delta_secs()).max(0.);
        }
        let want = if view.flash > 0. {
            &assets.flash
        } else {
            &assets.plain
        };
        if material.0 != *want {
            material.0 = want.clone();
        }
        *transform = slab_pose(
            origin.to_render(position.0) + Vec3::Y * CENTRE_LIFT,
            actor.facing,
            actor.guard_up,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_core::WorldPos;

    fn world() -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(RenderOrigin::default());
        app.init_resource::<Time>();
        app.add_message::<CombatImpact>();
        app.insert_resource(ShieldAssets {
            mesh: Handle::default(),
            plain: Handle::default(),
            flash: Handle::default(),
        });
        app.add_systems(Update, draw_slab);
        let mut actor = Combatant::on_team(1, 85.);
        actor.guard_up = true;
        let bearer = app
            .world_mut()
            .spawn((Shield, WorldPosition(WorldPos::new(0., 0., 0.)), actor))
            .id();
        (app, bearer)
    }

    /// The first update spawns the slab, the second poses it.
    fn slab(app: &mut App) -> Transform {
        app.update();
        app.update();
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<SlabView>>()
            .single(world)
            .copied()
            .expect("one slab")
    }

    #[test]
    fn the_slab_is_upright_exactly_while_the_guard_is_up() {
        let (mut app, bearer) = world();
        let up = slab(&mut app);
        assert!(up.rotation.mul_vec3(Vec3::Y).dot(Vec3::Y) > 0.999);
        app.world_mut()
            .get_mut::<Combatant>(bearer)
            .expect("actor")
            .guard_up = false;
        let down = slab(&mut app);
        assert!(
            down.rotation.mul_vec3(Vec3::Y).dot(Vec3::Y) < 0.5,
            "leaned well out"
        );
        assert!(down.translation.y < up.translation.y);
    }

    #[test]
    fn the_slab_follows_its_bearer_and_goes_with_it() {
        let (mut app, bearer) = world();
        let before = slab(&mut app);
        app.world_mut()
            .get_mut::<WorldPosition>(bearer)
            .expect("position")
            .0 = WorldPos::new(10., 0., 0.);
        let after = slab(&mut app);
        assert!((after.translation.x - before.translation.x - 10.).abs() < 0.01);
        app.world_mut().despawn(bearer);
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&SlabView>().iter(world).count(), 0);
    }

    #[test]
    fn a_dead_bearers_slab_is_hidden_until_it_lives_again() {
        let (mut app, bearer) = world();
        slab(&mut app);
        app.world_mut()
            .get_mut::<Combatant>(bearer)
            .expect("actor")
            .action = Action::Dead;
        slab(&mut app);
        let world = app.world_mut();
        let shown = *world
            .query_filtered::<&Visibility, With<SlabView>>()
            .single(world)
            .expect("one slab");
        assert_eq!(shown, Visibility::Hidden);
        world.get_mut::<Combatant>(bearer).expect("actor").action = Action::Ready;
        slab(&mut app);
        let world = app.world_mut();
        let shown = *world
            .query_filtered::<&Visibility, With<SlabView>>()
            .single(world)
            .expect("one slab");
        assert_eq!(shown, Visibility::Inherited);
    }

    #[test]
    fn a_guarded_blow_flashes_the_slab_and_the_plain_look_returns() {
        let (mut app, bearer) = world();
        let (plain, flash) = {
            let mut materials = app.world_mut().resource_mut::<ShieldAssets>();
            let plain = Handle::<StandardMaterial>::Uuid(
                bevy::asset::uuid::Uuid::from_u128(1),
                std::marker::PhantomData,
            );
            let flash = Handle::<StandardMaterial>::Uuid(
                bevy::asset::uuid::Uuid::from_u128(2),
                std::marker::PhantomData,
            );
            materials.plain = plain.clone();
            materials.flash = flash.clone();
            (plain, flash)
        };
        slab(&mut app);
        let attacker = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(bearer),
            from: strata_core::WorldPos::new(0., 0., 5.),
            point: strata_core::WorldPos::new(0., 0., 0.),
            melee: false,
            dealt: 1.,
            lethal: false,
            guard: GuardOutcome::Blocked,
            shot: None,
        });
        let material = |app: &mut App| {
            let world = app.world_mut();
            world
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<SlabView>>()
                .single(world)
                .expect("one slab")
                .0
                .clone()
        };
        app.update();
        assert_eq!(material(&mut app), flash);
        let mut time = app.world_mut().resource_mut::<Time>();
        time.advance_by(std::time::Duration::from_secs_f32(FLASH_SECONDS + 0.05));
        app.update();
        assert_eq!(material(&mut app), plain);
    }
}
