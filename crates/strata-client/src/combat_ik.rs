//! Feet on the ground: two-bone leg IK over the animated pose, solved in
//! render space from ground rays under each foot. The capsule stands on one
//! point, so on a terrace bank one foot would float and the other sink; the
//! pelvis drops to the lower foot and each leg bends to its own ground.
//!
//! Presentation only: the character controller, the sim and the clips are
//! untouched. The pose is solved after the animation and the other pose
//! overlays, before transform propagation, from the current frame's local
//! transforms, so it never lags the body it stands under.

use bevy::prelude::*;
use strata_physics::CharacterController;
use strata_physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata_sim::combat::{Action, Combatant};
use strata_sim::ride::Riding;

/// The bones of one leg, hip to toe.
#[derive(Clone, Copy)]
pub(crate) struct Leg {
    pub(crate) thigh: Entity,
    pub(crate) shin: Entity,
    pub(crate) foot: Entity,
}

/// A visual's legs, bound once the scene's bones are named.
#[derive(Component)]
pub(crate) struct Legs {
    pub(crate) actor: Entity,
    /// The visual's root, placed at the capsule's foot each frame.
    pub(crate) root: Entity,
    pub(crate) hips: Entity,
    pub(crate) sides: [Leg; 2],
    /// Smoothed ground offsets per foot and the pelvis drop, metres.
    pub(crate) lift: [f32; 2],
    pub(crate) drop: f32,
}

/// How far into a kneel a member is, `0..=1`: set by the field's motion while
/// they sample. The feet are theirs then, so the ground solve stands aside and
/// the kneel's fold runs after it.
#[derive(Component)]
pub(crate) struct Kneel(pub(crate) f32);

/// Furthest a foot is moved up or down from its animated height, metres:
/// enough for a terrace bank's edge, not enough to split the hips.
const REACH: f32 = 0.35;
/// Steepest sole tilt onto the ground normal, radians.
const SOLE_TILT: f32 = 0.5;
/// Smoothing time constant, seconds: ground rays change at region seams and
/// steps, feet should not pop with them.
const SMOOTHING: f32 = 0.06;

pub(super) fn install(app: &mut App) {
    app.add_systems(
        PostUpdate,
        place_feet
            .after(bevy::app::AnimationSystems)
            .after(super::combat_art::PoseOverlays)
            .after(crate::ClientSet::Follow)
            .before(bevy::transform::TransformSystems::Propagate),
    );
}

/// Compose the current frame's transforms from `root` down to `entity`.
fn global(
    root: Entity,
    entity: Entity,
    parents: &Query<&ChildOf>,
    transforms: &Query<&mut Transform>,
) -> Option<GlobalTransform> {
    let mut chain = vec![entity];
    let mut current = entity;
    while current != root {
        current = parents.get(current).ok()?.parent();
        chain.push(current);
    }
    let mut result = GlobalTransform::IDENTITY;
    for entity in chain.into_iter().rev() {
        result = result.mul_transform(*transforms.get(entity).ok()?);
    }
    Some(result)
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one pass: rays, pelvis drop, both legs, written back in bone-local space"
)]
pub(crate) fn place_feet(
    time: Res<Time>,
    spatial: SpatialQuery,
    actors: Query<(
        Entity,
        &Combatant,
        &CharacterController,
        Has<Riding>,
        Has<Kneel>,
    )>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
    mut legs: Query<&mut Legs>,
) {
    let excluded: Vec<Entity> = actors.iter().map(|(entity, ..)| entity).collect();
    let filter = SpatialQueryFilter::from_excluded_entities(excluded);
    let alpha = 1. - (-time.delta_secs() / SMOOTHING).exp();
    for mut legs in &mut legs {
        let Ok((_, actor, controller, riding, kneeling)) = actors.get(legs.actor) else {
            continue;
        };
        // Airborne, rolling or dead bodies keep the clip's feet, and so does
        // a rider or a kneeler: their feet are on a bike or folded, not on the ground below it.
        let active = controller.grounded
            && !riding
            && !kneeling
            && !matches!(actor.action, Action::Dodge | Action::Dead);
        let Some(root) = global(legs.root, legs.root, &parents, &transforms) else {
            continue;
        };
        let ground = root.translation().y;
        let mut targets = [0_f32; 2];
        let mut normals = [Vec3::Y; 2];
        for (index, leg) in legs.sides.iter().enumerate() {
            let Some(foot) = global(legs.root, leg.foot, &parents, &transforms) else {
                continue;
            };
            let foot = foot.translation();
            let from = Vec3::new(foot.x, ground + 1., foot.z);
            if let Some(hit) = spatial.cast_ray(from, Dir3::NEG_Y, 2.5, true, &filter) {
                let height = from.y - hit.distance;
                targets[index] = (height - ground).clamp(-REACH, REACH);
                normals[index] = hit.normal;
            }
        }
        if !active {
            targets = [0., 0.];
        }
        // The pelvis follows the lower foot; the other leg bends to its ground.
        let drop = targets[0].min(targets[1]).min(0.);
        legs.drop += (drop - legs.drop) * alpha;
        for (lift, target) in legs.lift.iter_mut().zip(targets) {
            *lift += (target - *lift) * alpha;
        }
        let drop = legs.drop;
        let lift = legs.lift;
        if drop.abs() < 0.002 && lift.iter().all(|lift| lift.abs() < 0.002) {
            continue;
        }
        // Lower the hips in their parent's frame.
        if let Ok(parent) = parents.get(legs.hips)
            && let Some(parent) = global(legs.root, parent.parent(), &parents, &transforms)
            && let Ok(mut hips) = transforms.get_mut(legs.hips)
        {
            let local = parent.affine().inverse().transform_vector3(Vec3::Y * drop);
            hips.translation += local;
        }
        for (index, leg) in legs.sides.iter().enumerate() {
            let (Some(thigh), Some(shin), Some(foot)) = (
                global(legs.root, leg.thigh, &parents, &transforms),
                global(legs.root, leg.shin, &parents, &transforms),
                global(legs.root, leg.foot, &parents, &transforms),
            ) else {
                continue;
            };
            let Ok(parent) = parents.get(leg.thigh) else {
                continue;
            };
            let Some(parent) = global(legs.root, parent.parent(), &parents, &transforms) else {
                continue;
            };
            let hip = thigh.translation();
            let knee = shin.translation();
            let ankle = foot.translation();
            // The foot keeps the clip's lift above its own ground; the pelvis
            // drop has already moved the hip, so the target is relative to it.
            let target = ankle + Vec3::Y * (lift[index] - drop);
            let Some(solved) = solve(hip, knee, ankle, target) else {
                continue;
            };
            let thigh_rotation = solved.thigh * thigh.rotation();
            let shin_rotation = solved.shin * shin.rotation();
            // The sole leans onto the slope, within a stance's tolerance.
            let tilt = Quat::from_rotation_arc(Vec3::Y, normals[index].normalize_or(Vec3::Y));
            let (tilt_axis, tilt_angle) = tilt.to_axis_angle();
            let tilt = Quat::from_axis_angle(tilt_axis, tilt_angle.min(SOLE_TILT));
            let foot_rotation = tilt * foot.rotation();
            if let Ok(mut bone) = transforms.get_mut(leg.thigh) {
                bone.rotation = parent.rotation().inverse() * thigh_rotation;
            }
            if let Ok(mut bone) = transforms.get_mut(leg.shin) {
                bone.rotation = thigh_rotation.inverse() * shin_rotation;
            }
            if let Ok(mut bone) = transforms.get_mut(leg.foot) {
                bone.rotation = shin_rotation.inverse() * foot_rotation;
            }
        }
    }
}

/// World-space rotation deltas for the thigh and the shin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Solved {
    pub(crate) thigh: Quat,
    pub(crate) shin: Quat,
}

/// Two-bone IK: rotate the thigh and the shin so the ankle reaches `target`,
/// keeping the knee in the plane the clip put it in. Returns `None` when the
/// chain is degenerate (zero-length bones).
pub(crate) fn solve(hip: Vec3, knee: Vec3, ankle: Vec3, target: Vec3) -> Option<Solved> {
    let a = (knee - hip).length();
    let b = (ankle - knee).length();
    if a < 1e-4 || b < 1e-4 {
        return None;
    }
    let reach = target - hip;
    let d = reach.length().clamp((a - b).abs() + 1e-3, a + b - 1e-3);
    let Ok(direction) = Dir3::new(reach) else {
        return None;
    };
    // The knee's bend plane: the clip's, or sideways for a straight leg.
    let current = (knee - hip).cross(ankle - hip);
    let normal = if current.length_squared() > 1e-6 {
        current.normalize()
    } else {
        let side = (knee - hip).cross(Vec3::Y);
        if side.length_squared() > 1e-6 {
            side.normalize()
        } else {
            Vec3::X
        }
    };
    // Law of cosines at the hip, with the knee on the clip's side of the reach.
    let cos_hip = ((a * a + d * d - b * b) / (2. * a * d)).clamp(-1., 1.);
    let hip_angle = cos_hip.acos();
    let candidates = [
        Quat::from_axis_angle(normal, hip_angle) * *direction,
        Quat::from_axis_angle(normal, -hip_angle) * *direction,
    ];
    let current_thigh = (knee - hip).normalize();
    let thigh_direction = candidates
        .into_iter()
        .max_by(|p, q| p.dot(current_thigh).total_cmp(&q.dot(current_thigh)))?;
    let new_knee = hip + thigh_direction * a;
    let shin_direction = (hip + *direction * d - new_knee).normalize_or(thigh_direction);
    Some(Solved {
        thigh: Quat::from_rotation_arc(current_thigh, thigh_direction),
        shin: Quat::from_rotation_arc((ankle - knee).normalize(), shin_direction),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(hip: Vec3, knee: Vec3, ankle: Vec3, solved: Solved) -> (Vec3, Vec3) {
        let knee2 = hip + solved.thigh * (knee - hip);
        let ankle2 = knee2 + solved.shin * (ankle - knee);
        (knee2, ankle2)
    }

    #[test]
    fn a_lowered_target_bends_the_knee_forward_and_reaches_it() {
        let hip = Vec3::new(0., 0.9, 0.);
        let knee = Vec3::new(0., 0.45, -0.02);
        let ankle = Vec3::new(0., 0.05, 0.);
        let target = ankle + Vec3::Y * 0.25;
        let solved = solve(hip, knee, ankle, target).expect("a leg");
        let (knee2, ankle2) = apply(hip, knee, ankle, solved);
        assert!(ankle2.distance(target) < 1e-3, "reached: {ankle2}");
        assert!(knee2.z < knee.z, "the knee bends the way the clip had it");
        assert!(
            ((knee2 - hip).length() - (knee - hip).length()).abs() < 1e-4,
            "bone lengths hold"
        );
    }

    #[test]
    fn an_unreachable_target_straightens_the_leg_toward_it() {
        let hip = Vec3::new(0., 0.9, 0.);
        let knee = Vec3::new(0., 0.45, -0.1);
        let ankle = Vec3::new(0., 0.05, 0.);
        let target = Vec3::new(0.3, -0.5, 0.);
        let solved = solve(hip, knee, ankle, target).expect("a leg");
        let (knee2, ankle2) = apply(hip, knee, ankle, solved);
        let reach = (knee - hip).length() + (ankle - knee).length();
        assert!(
            (ankle2.distance(hip) - reach).abs() < 2e-3,
            "fully extended"
        );
        let along = (ankle2 - hip).normalize().dot((target - hip).normalize());
        assert!(along > 0.999, "pointing at the target: {along}");
        assert!(knee2.is_finite());
    }
}
