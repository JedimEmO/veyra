//! The bike and its rider on screen (ticket 0009, step 2): the baked bike
//! stood under the rider at the hover height, leaning into a turn and
//! pitching with the slope, the rider lowered onto the saddle, and the
//! [`RideView`] the cameras ease by.
//!
//! All of it is a view over the sim's [`Riding`]: the hover is drawn and not
//! simulated (the capsule stays on the ground), so the feel constants here
//! are about how it looks and change no figure the sim reads. The vehicle's
//! own dials for the view (hover, boom, eye, seat) come from its file.
//!
//! The character art has no seated clip. The rider keeps the idle pose,
//! lowered so the hips sit on the saddle, and the thighs and shins are
//! folded over it (`pose_seated`); the weapon is hidden by `combat_art`.

use ashlar_bevy::prelude::{AshlarProp, MaterialLibraryAsset};
use bevy::prelude::*;
use strata_physics::CharacterController;
use strata_registry::vehicle::{MODELS_PATH, VehicleModels};
use strata_sim::ride::Riding;
use strata_sim::{LocalPlayer, PreviousWorldPosition, Registries, RenderOrigin, WorldPosition};

use crate::weapon_model::{Pieces, Standing};

/// Where the body's hips are over its feet, metres. This and the fold angles
/// below belong to the character rig and not to a vehicle: a property of the rig,
/// which the rider is lowered by so the hips sit on the saddle.
pub const HIP_M: f32 = 0.95;
/// How far the thighs are folded forward on the saddle, and the shins back,
/// radians.
const THIGH_FOLD: f32 = 1.15;
const SHIN_FOLD: f32 = 0.7;
/// How far the upper arms swing forward and the forearms bend to the bars,
/// radians, and how far the upper arms stand off the ribs. The rig's, like
/// the folds above.
const ARM_REACH: f32 = 0.9;
const FOREARM_BEND: f32 = 0.5;
const ARM_SPLAY: f32 = 0.12;

/// An arm bone of a visual, so a rider's hands can be put on the bars.
#[derive(Component)]
pub(crate) struct RiderArm {
    pub(crate) actor: Entity,
    pub(crate) upper: bool,
    pub(crate) left: bool,
}

/// The camera's pull onto the bike, shared by both cameras.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct RideView {
    /// 0 on foot, 1 fully on the bike; moves by `dt / look.ease_s`.
    pub blend: f32,
    /// The boom while mounted, metres.
    pub distance: f32,
    /// The eye over the ground while mounted, metres.
    pub eye: f32,
    /// How high the bike hovers, metres.
    pub hover_m: f32,
    /// Where the rider sits in the bike's frame.
    pub seat: Vec3,
    /// The vehicle's look dials, copied from its file while mounted.
    pub look: Look,
}

/// The vehicle file's `view` dials that shape the look, with the values the
/// courier bike ships, so a view not yet filled eases and bobs sensibly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The lean at the slow-turn rate, radians.
    pub max_lean: f32,
    /// The pitch limit, radians.
    pub max_pitch: f32,
    /// The bob, metres.
    pub bob_m: f32,
    /// The bob's period, seconds.
    pub bob_period_s: f32,
    /// The camera's pull, seconds.
    pub ease_s: f32,
    /// The lean and pitch follow rate, per second.
    pub follow: f32,
}

impl Look {
    /// The dials of a vehicle's `view`.
    #[must_use]
    pub fn of(view: &strata_registry::vehicle::View) -> Self {
        Self {
            max_lean: view.lean_deg.to_radians(),
            max_pitch: view.pitch_max,
            bob_m: view.bob_m,
            bob_period_s: view.bob_period_s,
            ease_s: view.ease_s,
            follow: view.follow,
        }
    }
}

/// A look with nothing in it: the vehicle file's dials are put in at the
/// mount, and nothing reads a look before then.
impl Default for Look {
    fn default() -> Self {
        Self {
            max_lean: 0.0,
            max_pitch: 0.0,
            bob_m: 0.0,
            bob_period_s: 1.0,
            ease_s: 1.0,
            follow: 1.0,
        }
    }
}

impl Default for RideView {
    fn default() -> Self {
        Self {
            blend: 0.0,
            distance: 0.0,
            eye: 0.0,
            hover_m: 0.0,
            seat: Vec3::ZERO,
            look: Look::default(),
        }
    }
}

impl RideView {
    /// The eased weight, smooth at both ends.
    #[must_use]
    pub fn weight(&self) -> f32 {
        self.blend * self.blend * (3.0 - 2.0 * self.blend)
    }

    /// `foot` towards `ride` by the eased weight.
    #[must_use]
    pub fn mix(&self, foot: f32, ride: f32) -> f32 {
        foot + (ride - foot) * self.weight()
    }

    /// Moves the blend towards mounted or on foot over `dt` seconds.
    pub fn ease(&mut self, riding: bool, dt: f32) {
        let step = dt / self.look.ease_s.max(1e-3);
        self.blend = if riding {
            (self.blend + step).min(1.0)
        } else {
            (self.blend - step).max(0.0)
        };
    }
}

/// The bob over the hover at `seconds`, metres.
#[must_use]
pub fn bob(look: &Look, seconds: f32) -> f32 {
    look.bob_m * (seconds * std::f32::consts::TAU / look.bob_period_s.max(1e-3)).sin()
}

/// How high the rider's feet are over the capsule's foot: the hover and the
/// bob, plus the seat over the hips. Negative would bury the rider.
#[must_use]
pub fn rider_lift(view: &RideView, seconds: f32) -> f32 {
    view.hover_m + bob(&view.look, seconds) + view.seat.y - HIP_M
}

/// The lean for a heading changing at `rate` radians a second, given the
/// slow-turn rate that is a full lean: into the turn, so a left turn (the
/// heading growing) rolls the bike to the left.
#[must_use]
pub fn lean_for(look: &Look, rate: f32, full_rate: f32) -> f32 {
    -(rate / full_rate.max(1e-3)).clamp(-1.0, 1.0) * look.max_lean
}

/// The nose pitch for a ground whose normal is `normal`, heading `heading`:
/// nose up on a rise.
#[must_use]
pub fn pitch_for(look: &Look, normal: Vec3, heading: f32) -> f32 {
    let forward = strata_sim::PlayerIntent::forward(heading);
    (-normal.dot(forward))
        .atan2(normal.y.max(1e-3))
        .clamp(-look.max_pitch, look.max_pitch)
}

/// The baked bike's index. Empty on a clone that has not run `just content`:
/// a plain box stands in.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct BikeModels(pub VehicleModels);

impl BikeModels {
    /// The index under `generated`, the content step's output root, as a
    /// source rooted there. A missing
    /// one is no error: it is said once at info and the box is drawn.
    #[must_use]
    pub fn read(generated: &strata_registry::files::Files) -> Self {
        let path = std::path::Path::new(MODELS_PATH);
        match generated
            .read_string(MODELS_PATH)
            .map_err(|error| error.to_string())
            .and_then(|text| VehicleModels::from_ron(&text).map_err(|error| error.to_string()))
        {
            Ok(set) => {
                tracing::info!(path = %path.display(), bikes = set.files.len(), "bike models");
                Self(set)
            }
            Err(error) => {
                tracing::info!(
                    path = %path.display(),
                    %error,
                    "no bike models: a plain box stands in until `just content` runs"
                );
                Self::default()
            }
        }
    }
}

/// A bike on screen under `actor`.
#[derive(Component, Debug)]
pub struct BikeVisual {
    actor: Entity,
    /// The heading last frame and the smoothed turn rate, radians a second.
    heading: f32,
    rate: f32,
    /// The smoothed lean and pitch, radians.
    lean: f32,
    pitch: f32,
    /// The baked prop is stood and judged once.
    judged: bool,
}

/// The stand-in box's material and mesh.
#[derive(Resource)]
struct BoxBike {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// `Startup`: the box.
fn load_box(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(BoxBike {
        mesh: meshes.add(Cuboid::new(0.8, 0.4, 2.2)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.45, 0.46),
            metallic: 0.3,
            perceptual_roughness: 0.5,
            ..default()
        }),
    });
}

/// `Update`: the camera's pull follows whether the local player rides.
pub fn follow_ride(
    time: Res<Time>,
    registries: Option<Res<Registries>>,
    players: Query<Option<&Riding>, With<LocalPlayer>>,
    mut view: ResMut<RideView>,
) {
    let riding = players.single().ok().flatten();
    if let Some(def) = riding
        .zip(registries.as_deref())
        .and_then(|(riding, registries)| registries.0.vehicles.get(&riding.vehicle))
    {
        view.distance = def.view.camera_distance;
        view.eye = def.view.camera_eye;
        view.hover_m = def.view.hover_m;
        view.seat = Vec3::from(def.view.seat);
        view.look = Look::of(&def.view);
    }
    view.ease(riding.is_some(), time.delta_secs());
}

/// `Update`: a bike for each rider that has none, and none for a rider who
/// has got off.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn dress_bikes(
    mut commands: Commands,
    server: Res<AssetServer>,
    models: Res<BikeModels>,
    plain: Res<BoxBike>,
    riders: Query<(Entity, &Riding)>,
    added: Query<Entity, Added<Riding>>,
    bikes: Query<(Entity, &BikeVisual)>,
) {
    for (bike, visual) in &bikes {
        if riders.get(visual.actor).is_err() {
            commands.entity(bike).despawn();
        }
    }
    for entity in &added {
        let Ok((_, riding)) = riders.get(entity) else {
            continue;
        };
        let root = commands
            .spawn((
                Name::new("Bike visual"),
                BikeVisual {
                    actor: entity,
                    heading: riding.heading,
                    rate: 0.0,
                    lean: 0.0,
                    pitch: 0.0,
                    judged: false,
                },
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        match models.0.file(&riding.vehicle) {
            Some(file) => {
                let library: Handle<MaterialLibraryAsset> =
                    server.load(format!("generated://{}", models.0.materials));
                commands.entity(root).with_child(AshlarProp {
                    prop: server.load(format!("generated://{file}")),
                    materials: library,
                });
            }
            None => stand_box(&mut commands, root, &plain),
        }
    }
}

/// The box on a bike root, its underside on the origin like the baked one.
fn stand_box(commands: &mut Commands, root: Entity, plain: &BoxBike) {
    commands.entity(root).with_child((
        Mesh3d(plain.mesh.clone()),
        MeshMaterial3d(plain.material.clone()),
        Transform::from_xyz(0.0, 0.35, 0.0),
    ));
    if let Ok(mut root) = commands.get_entity(root) {
        root.insert(Name::new("Bike visual (box)"));
    }
}

/// `Update`: a baked bike that failed to load is replaced by the box, once.
fn judge_bikes(
    mut commands: Commands,
    plain: Res<BoxBike>,
    pieces: Pieces,
    mut bikes: Query<(Entity, &mut BikeVisual)>,
) {
    for (bike, mut visual) in &mut bikes {
        if visual.judged {
            continue;
        }
        match pieces.standing(bike) {
            Standing::Loading => {}
            Standing::Ready => visual.judged = true,
            Standing::Failed => {
                visual.judged = true;
                commands.entity(bike).despawn_related::<Children>();
                stand_box(&mut commands, bike, &plain);
            }
        }
    }
}

/// `PostUpdate`, after the origin sync: each bike where its rider is, at
/// the hover height, leaning and pitched.
pub fn place_bikes(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    view: Res<RideView>,
    riders: Query<(
        &Riding,
        &WorldPosition,
        &PreviousWorldPosition,
        &CharacterController,
    )>,
    mut bikes: Query<(&mut BikeVisual, &mut Transform)>,
) {
    let dt = time.delta_secs().max(1e-4);
    let smooth = 1.0 - (-view.look.follow * dt).exp();
    for (mut visual, mut transform) in &mut bikes {
        let Ok((riding, position, previous, controller)) = riders.get(visual.actor) else {
            continue;
        };
        let rate = wrap(riding.heading - visual.heading) / dt;
        visual.heading = riding.heading;
        visual.rate += (rate - visual.rate) * smooth;
        let lean = lean_for(&view.look, visual.rate, riding.dials.turn.slow.to_radians());
        let pitch = pitch_for(&view.look, controller.ground_normal, riding.heading);
        visual.lean += (lean - visual.lean) * smooth;
        visual.pitch += (pitch - visual.pitch) * smooth;
        let ground = origin
            .to_render(previous.interpolate(position.0, fixed.overstep_fraction_f64()))
            - Vec3::Y * controller.half_height();
        transform.translation =
            ground + Vec3::Y * (view.hover_m + bob(&view.look, time.elapsed_secs()));
        // The recipe's nose is +z and a yaw faces -z, so a half turn.
        transform.rotation = Quat::from_rotation_y(riding.heading + std::f32::consts::PI)
            * Quat::from_rotation_z(visual.lean)
            * Quat::from_rotation_x(-visual.pitch);
    }
}

/// `value` wrapped to `(-pi, pi]`.
#[must_use]
pub fn wrap(value: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let a = value.rem_euclid(tau);
    if a > std::f32::consts::PI { a - tau } else { a }
}

/// `PostUpdate`, after the animation and the pose overlays, before the
/// transforms propagate: a rider's thighs folded forward and shins back, so
/// the legs sit either side of the saddle and not through the hull.
pub(crate) fn pose_seated(
    riders: Query<(), With<Riding>>,
    legs: Query<&crate::combat_ik::Legs>,
    mut bones: Query<&mut Transform>,
) {
    for legs in &legs {
        if riders.get(legs.actor).is_err() {
            continue;
        }
        for side in legs.sides {
            if let Ok(mut thigh) = bones.get_mut(side.thigh) {
                thigh.rotation = Quat::from_rotation_x(-THIGH_FOLD) * thigh.rotation;
            }
            if let Ok(mut shin) = bones.get_mut(side.shin) {
                shin.rotation = Quat::from_rotation_x(SHIN_FOLD) * shin.rotation;
            }
        }
    }
}

/// `PostUpdate`, with the legs: a rider's arms forward and down to where the
/// bars are, so from behind the rider holds the bike and not the idle pose's
/// raised rifle arms. An overlay on the animated pose, not a clip.
pub(crate) fn pose_arms(riders: Query<(), With<Riding>>, arms: Query<(&RiderArm, &mut Transform)>) {
    let mut arms = arms;
    for (arm, mut transform) in &mut arms {
        if riders.get(arm.actor).is_err() {
            continue;
        }
        let side = if arm.left { 1.0 } else { -1.0 };
        transform.rotation *= if arm.upper {
            Quat::from_rotation_z(side * ARM_SPLAY) * Quat::from_rotation_x(-ARM_REACH)
        } else {
            Quat::from_rotation_x(-FOREARM_BEND)
        };
    }
}

/// Registers the view and its easing; the cameras read it. Needs no assets,
/// so a client over a headless app has it too.
pub(crate) fn install_view(app: &mut App) {
    app.init_resource::<RideView>()
        .add_systems(Update, follow_ride);
}

/// Registers the bike and the rider's seat. Needs a renderer's assets.
pub(crate) fn install_art(app: &mut App) {
    app.init_resource::<BikeModels>()
        .add_systems(Startup, load_box)
        .add_systems(Update, (dress_bikes, judge_bikes).chain())
        .add_systems(PostUpdate, place_bikes.in_set(crate::ClientSet::Follow))
        .add_systems(
            PostUpdate,
            (pose_seated, pose_arms)
                .after(crate::combat_art::PoseOverlays)
                .before(bevy::transform::TransformSystems::Propagate),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        Look {
            max_lean: 20.0_f32.to_radians(),
            max_pitch: 0.45,
            bob_m: 0.03,
            bob_period_s: 2.6,
            ease_s: 0.5,
            follow: 8.0,
        }
    }

    #[test]
    fn the_pull_takes_half_a_second_each_way() {
        let mut view = RideView {
            look: look(),
            ..RideView::default()
        };
        for _ in 0..29 {
            view.ease(true, 1.0 / 60.0);
        }
        assert!(view.blend < 1.0, "{}", view.blend);
        for _ in 0..2 {
            view.ease(true, 1.0 / 60.0);
        }
        assert!((view.blend - 1.0).abs() < 1e-6);
        view.ease(false, 0.25);
        assert!((view.blend - 0.5).abs() < 1e-6);
        assert!((view.mix(3.8, 9.0) - 6.4).abs() < 1e-5, "smooth at half");
    }

    #[test]
    fn a_turn_leans_into_it_up_to_twenty_degrees() {
        let full = 150.0_f32.to_radians();
        let left = lean_for(&look(), full, full);
        assert!((left.abs() - 20.0_f32.to_radians()).abs() < 1e-6);
        assert!(left < 0.0, "a left turn rolls to the left side down");
        assert!((lean_for(&look(), -full, full) + left).abs() < 1e-6);
        assert!(lean_for(&look(), 10.0 * full, full).abs() <= look().max_lean + 1e-6);
        assert!(lean_for(&look(), 0.0, full).abs() < 1e-9);
    }

    #[test]
    fn a_rise_lifts_the_nose_and_a_dip_drops_it() {
        let tilt = |angle: f32, heading: f32| {
            let forward = strata_sim::PlayerIntent::forward(heading);
            // A rise: the normal leans back from the way it climbs.
            Vec3::new(0.0, angle.cos(), 0.0) - forward * angle.sin()
        };
        let up = pitch_for(&look(), tilt(0.2, 0.7), 0.7);
        assert!((up - 0.2).abs() < 1e-5, "{up}");
        assert!((pitch_for(&look(), tilt(-0.2, 0.7), 0.7) + 0.2).abs() < 1e-5);
        assert!((pitch_for(&look(), Vec3::Y, 1.0)).abs() < 1e-6);
    }

    #[test]
    fn the_hips_sit_on_the_saddle() {
        let view = RideView {
            hover_m: 0.9,
            seat: Vec3::new(0.0, 0.55, -0.1),
            look: look(),
            ..RideView::default()
        };
        let hips = rider_lift(&view, 0.0) + HIP_M;
        assert!((hips - (0.9 + 0.55)).abs() < 1e-6, "{hips}");
        assert!(bob(&look(), 0.65).abs() <= look().bob_m + 1e-6);
    }
}
