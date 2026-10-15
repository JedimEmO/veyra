//! Mouse-first combat view: direct aiming, an obstructed shoulder camera and feedback.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use strata_physics::avian3d::prelude::{
    Collider, ShapeCastConfig, SpatialQuery, SpatialQueryFilter,
};
use strata_sim::combat::{CombatImpact, CombatIntent, Combatant, ShotFired};
use strata_sim::progression::{Interact, Reach};
use strata_sim::{LocalPlayer, PlayerIntent, PreviousWorldPosition, RenderOrigin, WorldPosition};

use crate::{ClientSet, OrbitCamera};

/// Opt-in camera state, so connected play retains the existing orbit camera.
#[derive(Component, Debug)]
pub struct CombatCamera {
    distance: f32,
    shoulder: f32,
    pub(crate) base_fov: f32,
    /// A boom length that replaces the walking and aiming ones, for a still
    /// taken close to the character.
    pub hold: Option<f32>,
    /// Radians the camera is turned round the character from behind it,
    /// the player still facing where the view's own heading says, for a
    /// still taken from the side. Nought in play.
    pub yaw_offset: f32,
}

impl Default for CombatCamera {
    fn default() -> Self {
        Self {
            distance: 3.8,
            shoulder: 0.65,
            base_fov: 45_f32.to_radians(),
            hold: None,
            yaw_offset: 0.,
        }
    }
}

/// Local impulses cannot pause the simulation or change hit windows.
/// The camera reads the recoil and the hurt; the HUD reads the hit (the
/// marker round the reticle) and the hurt (the vignette).
#[derive(Resource, Default)]
pub(crate) struct Feedback {
    pub(crate) recoil: f32,
    pub(crate) hit: f32,
    pub(crate) hurt: f32,
    /// How shaken the camera is, `0..=1`: a hurt adds by its share of the
    /// member's pools and a heavy cut landed adds a little, and it runs
    /// down linearly in [`TRAUMA_SECS`]. Never read by the sim.
    pub(crate) trauma: f32,
    /// The latest hit killed: the marker wears danger rather than accent.
    pub(crate) lethal: bool,
    /// What the aim ray from the camera hits, if anything: the reticle
    /// turns warm over a hostile. Presentation only; the sim aims at the
    /// intent's point, never at this.
    pub(crate) under_reticle: Option<Entity>,
}

/// Whether the camera may shake: the settings' Camera shake switch, on
/// until the member turns it off.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraShake(pub bool);

impl Default for CameraShake {
    fn default() -> Self {
        Self(true)
    }
}

/// Seconds a full trauma takes to run down: under half a second, so a
/// hurt is felt and is gone before the next move.
const TRAUMA_SECS: f32 = 0.4;
/// Trauma a hurt adds per share of the member's pools it took: a blow of
/// a quarter of them shakes at half strength.
const HURT_TRAUMA_PER_SHARE: f32 = 2.;
/// Trauma a heavy cut landed adds.
const HEAVY_TRAUMA: f32 = 0.15;
/// The most the shake moves the camera, metres, and rolls it, radians,
/// at full trauma.
const SHAKE_OFFSET_M: f32 = 0.05;
const SHAKE_ROLL: f32 = 0.025;
/// The shake's two rates, radians a second: the offset's and the roll's.
const SHAKE_RATES: [f32; 3] = [57., 71., 43.];

/// `trauma` after `dt` seconds of running down.
fn trauma_after(trauma: f32, dt: f32) -> f32 {
    (trauma - dt / TRAUMA_SECS).max(0.)
}

/// The camera's offset (its own axes) and roll at `trauma` and time `t`:
/// the square of the trauma on a few unrelated sines, so a small hurt is a
/// tremor and a big one a jolt, and nought is nought.
#[must_use]
pub(crate) fn shake_pose(trauma: f32, t: f32) -> (Vec3, f32) {
    let strength = trauma.clamp(0., 1.).powi(2);
    let [a, b, c] = SHAKE_RATES;
    (
        Vec3::new((t * a).sin(), (t * b).sin(), 0.) * SHAKE_OFFSET_M * strength,
        (t * c).sin() * SHAKE_ROLL * strength,
    )
}

#[derive(Resource)]
struct Effects {
    cube: Handle<Mesh>,
    shot: Handle<StandardMaterial>,
}

/// Installs only the presentation adapter; simulation also works without this.
#[derive(Debug, Clone, Copy)]
pub struct CombatViewPlugin;

impl Plugin for CombatViewPlugin {
    fn build(&self, app: &mut App) {
        crate::combat_audio::install(app);
        crate::combat_fx::install(app);
        crate::combat_beam::install(app);
        crate::combat_shield::install(app);
        let input = (read_input, write_ability_press)
            .after(crate::input::write_player_intent)
            .in_set(ClientSet::Camera);
        #[cfg(feature = "ui")]
        let input = input.run_if(crate::ui::game_input_allowed);
        app.init_resource::<Feedback>()
            .init_resource::<CameraShake>()
            .add_systems(Startup, load_effects)
            .add_systems(Update, input)
            .add_systems(
                Update,
                probe_reticle.after(read_input).in_set(ClientSet::Camera),
            )
            .add_systems(
                Update,
                (feedback, draw_charge).chain().in_set(ClientSet::Hud),
            )
            .add_systems(PostUpdate, follow.in_set(ClientSet::Follow))
            .add_plugins(crate::combat_art::CombatArtPlugin);
        #[cfg(feature = "ui")]
        {
            app.add_systems(
                Update,
                clear_input.run_if(bevy::ecs::schedule::common_conditions::not(
                    crate::ui::game_input_allowed,
                )),
            );
            hud_only(app);
            crate::scope::install(app);
            crate::combat_text::install(app);
            crate::nameplates::install(app);
        }
    }
}

/// The combat HUD alone: the vitals cluster, the controls card, the hit
/// marker and the hurt vignette ([`crate::combat_hud`]), without the
/// camera, the physics queries or the art. The whole plugin needs a
/// renderer's assets; a layout test over the headless UI stack needs
/// only this.
#[cfg(feature = "ui")]
pub fn hud_only(app: &mut App) {
    app.init_resource::<Feedback>();
    crate::combat_hud::install(app);
}

/// Present where the platform grants the pointer lock only to a click (a
/// browser), so the combat camera asks for it after the first one. The
/// entry inserts it; nothing else decides the platform.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PointerLockOnClick;

#[allow(
    clippy::too_many_arguments,
    reason = "input combines camera, world query and independent button resources"
)]
fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    spatial: SpatialQuery,
    origin: Res<RenderOrigin>,
    reach: Res<Reach>,
    mut interactions: MessageWriter<Interact>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    lock_on_click: Option<Res<PointerLockOnClick>>,
    mut clicked: Local<bool>,
    mut cameras: Query<(&mut OrbitCamera, &Transform), With<CombatCamera>>,
    mut players: Query<
        (Entity, &mut PlayerIntent, &mut CombatIntent, &Combatant),
        (With<LocalPlayer>, Without<strata_sim::ScriptedIntent>),
    >,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let Ok((mut camera, transform)) = cameras.single_mut() else {
        return;
    };
    let Ok((entity, mut movement, mut input, actor)) = players.single_mut() else {
        return;
    };
    if !window.focused {
        *input = CombatIntent::default();
        movement.move_dir = Vec2::ZERO;
        movement.jump = false;
        movement.sprint = false;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        // The browser dropped the lock with the focus, and grants it again
        // only to a new click.
        *clicked = false;
        return;
    }
    // A browser grants the pointer lock only to a click, and rejects an
    // earlier request with an error; elsewhere there is nothing to wait for.
    // A screen (the pause) that released the lock needs a new click too.
    if cursor.grab_mode == CursorGrabMode::None {
        *clicked = false;
    }
    *clicked |= lock_on_click.is_none() || mouse.just_pressed(MouseButton::Left);
    if *clicked {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    // Turned slower through a scope by the factor the view is magnified.
    let sensitivity = camera.sensitivity * crate::scope::look_factor(actor);
    camera.yaw -= motion.delta.x * sensitivity;
    camera.pitch = (camera.pitch - motion.delta.y * sensitivity).clamp(-1.25, 1.15);
    movement.yaw = camera.yaw;
    input.fire = mouse.pressed(MouseButton::Left);
    input.aim = mouse.pressed(MouseButton::Right);
    input.dodge |= keys.just_pressed(KeyCode::AltLeft) || keys.just_pressed(KeyCode::ControlLeft);
    input.reload |= keys.just_pressed(KeyCode::KeyR);
    if keys.just_pressed(KeyCode::Digit1) {
        input.weapon = Some(0);
    } else if keys.just_pressed(KeyCode::Digit2) {
        input.weapon = Some(1);
    }
    // F uses what the sim says is within reach; the world answers the
    // message (a swap, a lesson), and later a screen opens on it.
    if keys.just_pressed(KeyCode::KeyF)
        && let Some((entity, kind)) = &reach.nearest
    {
        interactions.write(Interact {
            entity: *entity,
            kind: kind.clone(),
        });
    }
    let direction = Dir3::new_unchecked(camera.rotation() * Vec3::NEG_Z);
    let filter = SpatialQueryFilter::from_excluded_entities([entity]);
    let reach = probe_range(actor);
    let distance = spatial
        .cast_ray(transform.translation, direction, reach, true, &filter)
        .map_or(reach, |hit| hit.distance);
    input.aim_point = origin.to_world(transform.translation + direction * distance);
}

/// The keys of the bar's places, in the order the sim numbers them: the
/// three places of the weapon in hand, then the two utility places. The heavy
/// cut is no key of its own now; it is pressed from its place.
pub const ABILITY_KEYS: [KeyCode; 5] = [
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyT,
    KeyCode::Digit3,
    KeyCode::Digit4,
];

/// The place a key press this frame means, the first when two fall together.
#[must_use]
pub fn ability_press(keys: &ButtonInput<KeyCode>) -> Option<usize> {
    ABILITY_KEYS.iter().position(|key| keys.just_pressed(*key))
}

/// `Update`: a press of a place's key is an edge the sim asks its rule about.
fn write_ability_press(
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut CombatIntent, (With<LocalPlayer>, Without<strata_sim::ScriptedIntent>)>,
) {
    let Some(place) = ability_press(&keys) else {
        return;
    };
    for mut input in &mut players {
        input.ability = Some(place);
    }
}

/// How far the aim ray and the reticle's probe look at least, metres.
const PROBE_RANGE: f32 = 180.0;

/// How far the aim ray looks for the weapon in hand: never under the probe's
/// floor, and as far as a long gun's bolt flies, so the reticle turns warm
/// over a hostile the bolt can still reach.
fn probe_range(actor: &Combatant) -> f32 {
    PROBE_RANGE.max(actor.weapon_spec().range)
}

/// `Update`: what the camera's centre ray hits, for the reticle's tint.
///
/// Its own cast rather than a field [`read_input`] fills: that system
/// stands down for a scripted player and while a screen is up, and the
/// reticle should tell the truth in a scripted capture too. The cast is
/// the same ray from the same transform, one more a frame.
fn probe_reticle(
    spatial: SpatialQuery,
    mut feedback: ResMut<Feedback>,
    cameras: Query<&Transform, With<CombatCamera>>,
    players: Query<(Entity, &Combatant), With<LocalPlayer>>,
) {
    let (Ok(transform), Ok((player, actor))) = (cameras.single(), players.single()) else {
        feedback.under_reticle = None;
        return;
    };
    let direction = Dir3::new_unchecked(transform.rotation * Vec3::NEG_Z);
    let filter = SpatialQueryFilter::from_excluded_entities([player]);
    let hit = spatial
        .cast_ray(
            transform.translation,
            direction,
            probe_range(actor),
            true,
            &filter,
        )
        .map(|hit| hit.entity);
    if feedback.under_reticle != hit {
        feedback.under_reticle = hit;
    }
}

#[cfg(feature = "ui")]
fn clear_input(mut actors: Query<(&mut CombatIntent, &mut Combatant), With<LocalPlayer>>) {
    for (mut input, mut actor) in &mut actors {
        *input = CombatIntent::default();
        actor.clear_buffer();
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the camera excludes actor colliders while following interpolated world coordinates"
)]
fn follow(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    feedback: Res<Feedback>,
    players: Query<
        (
            Entity,
            &WorldPosition,
            &PreviousWorldPosition,
            &Combatant,
            Option<&strata_physics::CharacterController>,
        ),
        With<LocalPlayer>,
    >,
    view: Option<Res<crate::ride_art::RideView>>,
    actors: Query<Entity, With<Combatant>>,
    mut cameras: Query<(
        &OrbitCamera,
        &mut CombatCamera,
        &mut Transform,
        &mut Projection,
    )>,
) {
    let Ok((_, position, previous, actor, controller)) = players.single() else {
        return;
    };
    let world = previous.interpolate(position.0, fixed.overstep_fraction_f64());
    // Pulled back and up onto the bike by the vehicle's own boom and eye, and
    // eased both ways. The eye is over the ground, the anchor over the
    // capsule's centre.
    let view = view.as_deref().filter(|_| controller.is_some());
    let on_foot = 0.60;
    let lift = match (view, controller) {
        (Some(view), Some(controller)) => view.mix(on_foot, view.eye - controller.half_height()),
        _ => on_foot,
    };
    let anchor = origin.to_render(world) + Vec3::Y * lift;
    let alpha = 1. - (-14. * time.delta_secs()).exp();
    let scoped = crate::scope::scoped_zoom(actor);
    for (orbit, mut camera, mut transform, mut projection) in &mut cameras {
        let walking = if actor.aiming { 2.15 } else { 3.8 };
        let distance = camera
            .hold
            .unwrap_or_else(|| view.map_or(walking, |view| view.mix(walking, view.distance)));
        let shoulder = if actor.aiming { 0.55 } else { 0.65 };
        if scoped.is_some() {
            // At the sight: the boom is nought, and it eases back out from
            // there when the scope is left.
            camera.distance = 0.;
            camera.shoulder = 0.;
        } else {
            camera.distance += (distance - camera.distance) * alpha;
            camera.shoulder += (shoulder - camera.shoulder) * alpha;
        }
        let rotation = Quat::from_rotation_y(camera.yaw_offset) * orbit.rotation();
        let desired = rotation * Vec3::new(camera.shoulder, 0.0, camera.distance);
        if scoped.is_some() {
            transform.translation = anchor;
        } else {
            let direction = Dir3::new_unchecked(desired.normalize());
            let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
            let distance = spatial
                .cast_shape(
                    &Collider::sphere(0.18),
                    anchor,
                    Quat::IDENTITY,
                    direction,
                    &ShapeCastConfig::from_max_distance(desired.length()),
                    &filter,
                )
                .map_or(desired.length(), |hit| (hit.distance - 0.06).max(0.05));
            transform.translation = anchor + direction * distance;
        }
        let (shake, roll) = shake_pose(feedback.trauma, time.elapsed_secs());
        transform.translation += rotation * shake;
        transform.rotation = rotation
            * Quat::from_rotation_x(feedback.recoil * 0.012)
            * Quat::from_rotation_z(feedback.hurt * 0.008 + roll);
        if let Projection::Perspective(p) = &mut *projection {
            let desired = crate::scope::view_fov(camera.base_fov, actor.aiming, scoped);
            p.fov += (desired - p.fov) * alpha;
        }
    }
}

fn feedback(
    time: Res<Time>,
    mut events: MessageReader<CombatImpact>,
    mut shots: MessageReader<ShotFired>,
    mut feedback: ResMut<Feedback>,
    shake: Option<Res<CameraShake>>,
    local: Query<(Entity, &Combatant), With<LocalPlayer>>,
) {
    let shaking = shake.is_none_or(|shake| shake.0);
    let pools = local
        .single()
        .ok()
        .map(|(_, actor)| actor.with_actor(|table| table.health) + actor.shield_capacity);
    let heavy = local
        .single()
        .is_ok_and(|(_, actor)| actor.action == strata_sim::combat::Action::Heavy);
    let local = local.single().ok().map(|(entity, _)| entity);
    feedback.trauma = if shaking {
        trauma_after(feedback.trauma, time.delta_secs())
    } else {
        0.
    };
    let decay = (-time.delta_secs() * 18.).exp();
    feedback.recoil *= decay;
    feedback.hit *= decay;
    feedback.hurt *= decay;
    for shot in shots.read() {
        if Some(shot.attacker) == local {
            feedback.recoil = (feedback.recoil + 0.7).min(1.5);
        }
    }
    for event in events.read() {
        if Some(event.attacker) == local && event.victim.is_some() {
            feedback.hit = 1.;
            feedback.lethal = event.lethal;
            if shaking && heavy && event.melee {
                feedback.trauma = (feedback.trauma + HEAVY_TRAUMA).min(1.);
            }
        }
        if local.is_some()
            && event.victim == local
            && event.guard != strata_sim::combat::GuardOutcome::Parried
        {
            feedback.hurt = 1.;
            if shaking && let Some(pools) = pools.filter(|pools| *pools > 0.) {
                feedback.trauma =
                    (feedback.trauma + event.dealt / pools * HURT_TRAUMA_PER_SHARE).min(1.);
            }
        }
    }
}

fn load_effects(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut material = |colour: Color| {
        materials.add(StandardMaterial {
            base_color: colour,
            emissive: colour.to_linear() * 3.,
            unlit: true,
            ..default()
        })
    };
    commands.insert_resource(Effects {
        cube: meshes.add(Cuboid::new(1., 1., 1.)),
        shot: material(Color::srgb(1., 0.65, 0.2)),
    });
}

#[derive(Component)]
struct ChargeGlow(Entity);

/// How much larger the glow at a marksman's muzzle is in its lock. Seen
/// through a scope its beam points at the eye and is a dot, so the lock
/// needs a cue that reads end on: the glow flares when the aim stops
/// following.
const LOCK_FLARE: f32 = 2.5;

/// The side of the glow's cube for a charge, flared in a lock.
fn glow_size(charge: f32, locked: bool) -> f32 {
    let size = 0.05 + charge * 0.22;
    if locked { size * LOCK_FLARE } else { size }
}

fn draw_charge(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    effects: Res<Effects>,
    added: Query<Entity, Added<Combatant>>,
    actors: Query<(&WorldPosition, &Combatant, Has<crate::combat_beam::AimBeam>)>,
    mut glows: Query<(Entity, &ChargeGlow, &mut Transform, &mut Visibility)>,
) {
    let tables = strata_sim::combat_tables::current();
    for actor in &added {
        commands.spawn((
            ChargeGlow(actor),
            Mesh3d(effects.cube.clone()),
            MeshMaterial3d(effects.shot.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    for (entity, glow, mut transform, mut visibility) in &mut glows {
        let Ok((position, actor, beam)) = actors.get(glow.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        *visibility = if actor.charge > 0. {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        transform.translation = origin.to_render(position.0)
            + Vec3::Y * 0.45
            + PlayerIntent::forward(actor.facing) * 0.6
            + PlayerIntent::right(actor.facing) * 0.2;
        let locked = beam && tables.opponents.marksman.locked(actor.charge);
        transform.scale = Vec3::splat(glow_size(actor.charge, locked));
    }
}

#[cfg(all(test, feature = "ui"))]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use strata_sim::combat::Action;

    fn pressed(keys: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut input = ButtonInput::<KeyCode>::default();
        for key in keys {
            input.press(*key);
        }
        input
    }

    #[test]
    fn the_glow_flares_in_a_lock_and_grows_with_the_charge_otherwise() {
        assert!(glow_size(0.9, false) > glow_size(0.2, false));
        assert!((glow_size(0.9, true) - glow_size(0.9, false) * LOCK_FLARE).abs() < 1e-6);
    }

    #[test]
    fn each_place_has_its_key_and_the_heavy_edge_has_none() {
        for (place, key) in ABILITY_KEYS.into_iter().enumerate() {
            assert_eq!(ability_press(&pressed(&[key])), Some(place), "{key:?}");
        }
        assert_eq!(ability_press(&pressed(&[KeyCode::KeyW])), None);
        assert_eq!(
            ability_press(&pressed(&[KeyCode::KeyT, KeyCode::KeyE])),
            Some(1),
            "the first place when two fall together"
        );
        let mut world = World::new();
        world.insert_resource(pressed(&[KeyCode::KeyQ]));
        let player = world.spawn((LocalPlayer, CombatIntent::default())).id();
        world.run_system_once(write_ability_press).expect("runs");
        let input = world.get::<CombatIntent>(player).expect("the player");
        assert_eq!(input.ability, Some(0));
        assert!(!input.heavy, "Q is the first place, not the heavy edge");
    }

    #[cfg(feature = "ui")]
    #[test]
    #[allow(clippy::unwrap_used, reason = "the scenario owns its player")]
    fn nothing_is_written_while_a_screen_is_up() {
        use bevy::ecs::schedule::IntoScheduleConfigs;
        use slotted::ui::{Presentation, ScreenKind, ScreenStack, StackEntry};
        let mut app = App::new();
        app.insert_resource(pressed(&[KeyCode::KeyE]))
            .init_resource::<ScreenStack>()
            .add_systems(
                Update,
                write_ability_press.run_if(crate::ui::game_input_allowed),
            );
        let player = app
            .world_mut()
            .spawn((LocalPlayer, CombatIntent::default()))
            .id();
        let root = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<ScreenStack>()
            .entries_mut()
            .push(StackEntry {
                root,
                kind: ScreenKind::new("slotted:pause"),
                presentation: Presentation::default(),
                menu: None,
                focus: None,
            });
        app.update();
        assert_eq!(
            app.world().get::<CombatIntent>(player).unwrap().ability,
            None
        );
        app.world_mut()
            .resource_mut::<ScreenStack>()
            .entries_mut()
            .clear();
        app.update();
        assert_eq!(
            app.world().get::<CombatIntent>(player).unwrap().ability,
            Some(1)
        );
    }

    #[test]
    #[allow(
        clippy::unwrap_used,
        reason = "missing input resources or entities fail this scenario"
    )]
    fn edges_latch_until_tick_and_focus_loss_releases_held_input() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_sim::SimPlugin,
            strata_stream::StreamPlugin,
            strata_physics::PhysicsPlugin,
        ));
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>();
        app.finish();
        app.cleanup();
        for _ in 0..4 {
            app.update();
        }
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut().spawn((
            CombatCamera::default(),
            OrbitCamera::default(),
            Transform::default(),
        ));
        let actor = app
            .world_mut()
            .spawn((
                LocalPlayer,
                PlayerIntent::default(),
                Combatant::default().carrying(
                    [
                        Some(strata_sim::combat::Weapon::Blade),
                        Some(strata_sim::combat::Weapon::Carbine),
                    ],
                    0,
                ),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().run_system_once(read_input).unwrap();
        app.world_mut()
            .run_system_once(write_ability_press)
            .unwrap();
        let intent = app.world().get::<CombatIntent>(actor).unwrap();
        assert_eq!(intent.ability, Some(0), "Q is the first place");
        assert!(!intent.heavy, "and no longer the heavy edge");
        assert!(
            intent.fire && !intent.light,
            "the left button is the trigger whatever is in hand, and E no longer cuts"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut().run_system_once(read_input).unwrap();
        assert!(
            app.world().get::<CombatIntent>(actor).unwrap().ability == Some(0),
            "a render frame cannot erase the edge before the tick"
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut().run_system_once(read_input).unwrap();
        let input = app.world().get::<CombatIntent>(actor).unwrap();
        assert!(!input.fire && input.ability.is_none());
        assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
    }

    /// The HUD's half of combat over a bare app: the feedback, the
    /// cluster, the overlays and the floating text, no renderer.
    fn hud_app() -> (App, Entity, Entity) {
        use slotted::ui::def::Tags;
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin)
            .add_message::<CombatImpact>()
            .add_message::<ShotFired>()
            .init_resource::<RenderOrigin>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(crate::ui::fonts::UiFonts {
                body: Handle::default(),
                display: Handle::default(),
            })
            .add_systems(Update, feedback.before(ClientSet::Hud));
        hud_only(&mut app);
        crate::combat_text::install(&mut app);
        let local = app
            .world_mut()
            .spawn((
                LocalPlayer,
                Name::new("Tovan Reyes"),
                WorldPosition::new(0., 0., 0.),
                Combatant::default(),
            ))
            .id();
        let hostile = app
            .world_mut()
            .spawn((WorldPosition::new(0., 0., 10.), Combatant::on_team(1, 85.)))
            .id();
        app.world_mut()
            .spawn(Tags::new().with(Tags::TEST_ID, crate::combat_hud::Meter::Health.row()));
        app.update();
        (app, local, hostile)
    }

    /// The strongest red in the vignette, or `None` while it is hidden.
    fn vignette(app: &mut App) -> Option<f32> {
        use bevy::ui::{BackgroundGradient, Gradient};
        let world = app.world_mut();
        let mut query = world.query::<(&Name, &Visibility, &BackgroundGradient)>();
        let (_, visibility, gradient) = query
            .iter(world)
            .find(|(name, ..)| name.as_str() == "hud.vignette")?;
        if *visibility == Visibility::Hidden {
            return None;
        }
        gradient
            .0
            .iter()
            .filter_map(|g| match g {
                Gradient::Radial(r) => r.stops.iter().map(|s| s.color.alpha()).reduce(f32::max),
                _ => None,
            })
            .reduce(f32::max)
    }

    /// The text of every node named `name` that is not hidden.
    fn shown(app: &mut App, name: &str) -> Vec<Entity> {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &Name, &Visibility)>();
        query
            .iter(world)
            .filter(|(_, n, v)| n.as_str() == name && **v != Visibility::Hidden)
            .map(|(e, ..)| e)
            .collect()
    }

    fn hurt_by(app: &mut App, from: Entity, to: Entity, dealt: f32) {
        app.world_mut().write_message(CombatImpact {
            attacker: from,
            victim: Some(to),
            from: strata_core::WorldPos::new(0., 0., 10.),
            point: strata_core::WorldPos::new(0., 0., 0.),
            melee: false,
            dealt,
            lethal: false,
            guard: strata_sim::combat::GuardOutcome::Open,
            shot: None,
        });
        app.update();
    }

    #[test]
    fn a_hurt_shakes_the_camera_by_its_share_and_the_switch_stops_it() {
        let (mut app, local, hostile) = hud_app();
        hurt_by(&mut app, hostile, local, 12.);
        let small = app.world().resource::<Feedback>().trauma;
        assert!(small > 0.05, "a hurt shakes: {small}");
        hurt_by(&mut app, hostile, local, 40.);
        let big = app.world().resource::<Feedback>().trauma;
        assert!(big > small, "a bigger share shakes more");
        // A blow the member struck shakes nothing.
        let (mut app, local, hostile) = hud_app();
        hurt_by(&mut app, local, hostile, 30.);
        assert!(app.world().resource::<Feedback>().trauma.abs() < f32::EPSILON);
        // Switched off, a hurt leaves the camera alone and the pose is still.
        let (mut app, local, hostile) = hud_app();
        app.insert_resource(CameraShake(false));
        hurt_by(&mut app, hostile, local, 40.);
        let off = app.world().resource::<Feedback>().trauma;
        assert!(off.abs() < f32::EPSILON, "switched off: {off}");
        let (offset, roll) = shake_pose(off, 1.3);
        assert!(offset.length() < f32::EPSILON && roll.abs() < f32::EPSILON);
    }

    #[test]
    fn trauma_runs_down_to_nought_in_under_half_a_second_and_the_pose_is_bounded() {
        const { assert!(TRAUMA_SECS < 0.5) };
        assert!(trauma_after(1., TRAUMA_SECS * 0.5) > 0.);
        assert!(trauma_after(1., TRAUMA_SECS).abs() < f32::EPSILON);
        for step in 0..200_u16 {
            let (offset, roll) = shake_pose(1., f32::from(step) * 0.013);
            assert!(offset.length() <= SHAKE_OFFSET_M * 1.5 && roll.abs() <= SHAKE_ROLL);
        }
    }

    #[test]
    fn a_hurt_raises_its_number_and_reddens_the_edge() {
        let (mut app, local, hostile) = hud_app();
        assert_eq!(vignette(&mut app), None, "clear at full health");
        app.world_mut().write_message(CombatImpact {
            attacker: hostile,
            victim: Some(local),
            from: strata_core::WorldPos::new(0., 0., 10.),
            point: strata_core::WorldPos::new(0., 0., 0.),
            melee: false,
            dealt: 12.,
            lethal: false,
            guard: strata_sim::combat::GuardOutcome::Open,
            shot: None,
        });
        app.update();
        let world = app.world_mut();
        let numbers: Vec<String> = world
            .query::<(&Name, &Text)>()
            .iter(world)
            .filter(|(name, _)| name.as_str() == "hud.combat_text")
            .map(|(_, text)| text.0.clone())
            .collect();
        assert_eq!(numbers, vec!["-12".to_owned()]);
        let alpha = vignette(&mut app).unwrap_or(0.0);
        assert!(alpha > 0.3, "the edge reddened: {alpha}");
    }

    /// The reticle's colour: its parts share one.
    fn reticle_colour(app: &mut App) -> Option<Color> {
        let reticle = *shown(app, "hud.reticle").first()?;
        let world = app.world();
        let first = *world.get::<Children>(reticle)?.first()?;
        world.get::<BackgroundColor>(first).map(|b| b.0)
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
    fn the_reticle_warms_over_a_standing_hostile() {
        use crate::combat_hud::WARM;
        let (mut app, _, hostile) = hud_app();
        let rest = reticle_colour(&mut app).unwrap();
        assert_ne!(rest, WARM);
        app.world_mut().resource_mut::<Feedback>().under_reticle = Some(hostile);
        app.update();
        assert_eq!(reticle_colour(&mut app), Some(WARM));
        app.world_mut()
            .get_mut::<Combatant>(hostile)
            .unwrap()
            .action = Action::Dead;
        app.update();
        assert_eq!(
            reticle_colour(&mut app),
            Some(rest),
            "a body is not a threat"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
    fn low_health_throbs_and_the_fall_brings_the_card() {
        let (mut app, local, _) = hud_app();
        assert_eq!(
            shown(&mut app, "hud.reticle").len(),
            1,
            "standing: the reticle"
        );
        assert!(shown(&mut app, "hud.down").is_empty());
        {
            let mut actor = app.world_mut().get_mut::<Combatant>(local).unwrap();
            actor.health *= 0.2;
        }
        app.update();
        let alpha = vignette(&mut app).unwrap_or(0.0);
        assert!(alpha >= 0.10 - 1e-4, "the pulse holds the edge: {alpha}");
        {
            let mut actor = app.world_mut().get_mut::<Combatant>(local).unwrap();
            actor.health = 0.;
            actor.action = Action::Dead;
        }
        app.update();
        assert_eq!(shown(&mut app, "hud.down").len(), 1, "down: the card");
        assert!(shown(&mut app, "hud.reticle").is_empty(), "and no reticle");
        app.world_mut().get_mut::<Combatant>(local).unwrap().action = Action::Ready;
        app.update();
        assert!(
            shown(&mut app, "hud.down").is_empty(),
            "gone the frame they stand"
        );
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "the scenario owns this actor")]
    fn opening_a_menu_discards_a_buffered_followup() {
        let mut actor = Combatant::default();
        let mut input = CombatIntent {
            light: true,
            ..default()
        };
        let mut motion = strata_sim::combat::CombatMotion::default();
        actor.step(&mut input, &PlayerIntent::default(), 0.10, &mut motion);
        input.light = true;
        actor.step(&mut input, &PlayerIntent::default(), 0.01, &mut motion);
        let sequence = actor.sequence;
        let mut world = World::new();
        let entity = world.spawn((LocalPlayer, actor, input)).id();
        world.run_system_once(clear_input).unwrap();
        let mut actor = world.get_mut::<Combatant>(entity).unwrap();
        for _ in 0..40 {
            actor.step(
                &mut CombatIntent::default(),
                &PlayerIntent::default(),
                0.01,
                &mut motion,
            );
        }
        assert_eq!(actor.sequence, sequence);
        assert_eq!(actor.action, Action::Ready);
    }
}
