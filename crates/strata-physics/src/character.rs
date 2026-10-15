//! The character: a kinematic capsule moved by its own move and slide.
//!
//! Avian 0.7 has no character controller component; it has the
//! [`MoveAndSlide`] system parameter, a sweep-and-project loop over the
//! collider tree. This module wraps it in the rules a walking character
//! needs: gravity and a jump, a slope limit that turns steep ground into a
//! wall, a step up over small ledges, a snap down so walking downhill stays
//! in contact, and water: the sea's, a river's or a lake's at the
//! character's point ([`strata_sim::water_here`], ticket 0005, step 3),
//! which slows a wader by its depth, floats a swimmer and carries both with
//! its flow. The body is kinematic, so avian never moves it:
//! [`step_characters`] computes the new position and writes both avian's
//! `Position` and the sim's `WorldPosition`.
//!
//! Everything here runs in render space in `f32` and is converted at the
//! end (ADR 0002). In a single process the character is the primary
//! observer and so always within the rebase distance of the origin; on a
//! dedicated server one origin follows the first player and the others are
//! stepped at their distance from it, which holds to millimetres at fifty
//! kilometres (ADR 0004, PLAN section 6).
//!
//! # Why the ground is waited for
//!
//! The ground streams in. A character spawned before its region's collider
//! has arrived would fall through the world, so a character over no
//! collider at all (a downward ray finds nothing) holds still until there
//! is one. That covers spawning, a stall in the streamer and a teleport.

pub mod ride;

use core::time::Duration;

use avian3d::prelude::{
    Collider, MoveAndSlide, MoveAndSlideConfig, MoveAndSlideHitResponse, Position, RigidBody,
    Rotation, ShapeCastConfig, SpatialQueryFilter,
};
use bevy::ecs::prelude::*;
use bevy::math::{Dir3, Quat, Vec3};
use bevy::time::{Fixed, Time};
use bevy::transform::components::Transform;
use strata_sim::combat::CombatMotion;
use strata_sim::ride::Riding;
use strata_sim::water::water_of;
use strata_sim::{
    PlayerCommand, PlayerIntent, RenderOrigin, WorldPosition, WorldSourceRes, water_here,
};

/// `Position = origin.to_render(WorldPosition)` for characters only, after
/// [`step_characters`] wrote their `WorldPosition`, so a character's
/// `Position` is current for the rest of the frame. Characters only,
/// because a dynamic body's `WorldPosition` is stale at this point (the
/// step moved its `Position` and the writeback has not run yet), and the
/// crate-wide sync here would put every falling body back where it was.
pub fn sync_characters(
    origin: Res<RenderOrigin>,
    mut characters: Query<(&WorldPosition, &mut Position), With<CharacterController>>,
) {
    for (world, mut position) in &mut characters {
        let render = origin.to_render(world.0);
        if position.0 != render {
            position.0 = render;
        }
    }
}

/// How far down a character looks for any ground at all before it agrees
/// to fall. Longer than any drop in the world.
pub const GROUND_PROBE: f32 = 4_000.0;

/// Falling faster than this is capped: a character that fell for a long
/// time would otherwise sweep kilometres per tick.
pub const TERMINAL_SPEED: f32 = 60.0;

/// The character's shape, tuning and state. The entity is a kinematic
/// body; the capsule collider is inserted from the dimensions by
/// [`init_characters`] on the first tick.
///
/// `Position` is the capsule's centre, [`Self::half_height`] above the
/// feet. A `Transform` is required because avian's collider hook scales the
/// shape by the entity's `GlobalTransform` on insert, to zero without one;
/// the sim's origin sync writes its translation like any other.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(RigidBody = RigidBody::Kinematic, Position, Rotation, Transform)]
pub struct CharacterController {
    /// Acceleration-limited horizontal motion, enabled for the combat study.
    pub responsive: bool,
    /// Motor velocity before projection onto the ground.
    pub horizontal_velocity: Vec3,
    /// Authored action travel still passes through move-and-slide.
    pub action_velocity: Option<Vec3>,
    /// Grace time left after losing walkable ground.
    pub coyote_left: f32,
    /// A recent jump waits briefly for landing.
    pub jump_buffer_left: f32,
    /// Capsule radius, metres.
    pub radius: f32,
    /// Total capsule height, hemispheres included, metres.
    pub height: f32,
    /// Ground speed, metres per second.
    pub walk_speed: f32,
    /// Ground speed with sprint held.
    pub sprint_speed: f32,
    /// Speed multiplier with the centre under the water's surface.
    pub swim_factor: f32,
    /// How deep the water over the feet has to be before walking is slowed
    /// all the way to [`swim_factor`](Self::swim_factor), metres: about the
    /// waist, where the centre goes under and the swim takes over, so the
    /// speed falls smoothly from the bank into deep water.
    pub wade_depth: f32,
    /// The share of the water's own speed that carries a body wholly in it.
    /// A wader is carried by the share of its height under water, so a
    /// creek a hand deep nudges a walker by centimetres a second and a
    /// river at the waist is felt; a swimmer drifts with most of the flow.
    pub current_share: f32,
    /// The least speed a fall carries a body in it at, metres a second:
    /// the mean speed of the river above it would let a swimmer stand on the
    /// lip of a waterfall. With [`current_share`](Self::current_share) it is
    /// above the sprinting swim's speed, so nobody swims up a fall, and a
    /// fall holds nobody up: its water carries, it does not float.
    pub fall_speed: f32,
    /// Upward speed a jump starts with.
    pub jump_speed: f32,
    /// Downward acceleration while airborne. Above Earth's, as games do,
    /// so a jump is quick rather than floaty.
    pub gravity: f32,
    /// Ground steeper than this, in radians from horizontal, is a wall.
    pub max_slope: f32,
    /// Ledges up to this high are stepped over.
    pub step_height: f32,
    /// How far down a grounded character is pulled back to the ground
    /// after a move, so walking down a slope does not become a series of
    /// small falls.
    pub snap_distance: f32,
    /// How far below the water's surface the capsule's centre floats. One
    /// metre keeps the head just under the surface.
    pub float_depth: f32,
    /// Vertical speed, metres per second, positive up. State.
    pub vertical_velocity: f32,
    /// Standing on walkable ground as of the last step. State.
    pub grounded: bool,
    /// Floating in water as of the last step. State.
    pub swimming: bool,
    /// The normal of the ground stood on, or up. State.
    pub ground_normal: Vec3,
}

impl Default for CharacterController {
    fn default() -> Self {
        Self {
            responsive: false,
            horizontal_velocity: Vec3::ZERO,
            action_velocity: None,
            coyote_left: 0.0,
            jump_buffer_left: 0.0,
            radius: 0.35,
            height: 1.7,
            walk_speed: 5.0,
            sprint_speed: 9.0,
            swim_factor: 0.5,
            wade_depth: 0.85,
            current_share: 0.8,
            fall_speed: 6.0,
            jump_speed: 4.5,
            gravity: 15.0,
            max_slope: 50.0f32.to_radians(),
            step_height: 0.5,
            snap_distance: 0.5,
            float_depth: 1.0,
            vertical_velocity: 0.0,
            grounded: false,
            swimming: false,
            ground_normal: Vec3::Y,
        }
    }
}

impl CharacterController {
    /// Distance from the capsule's centre to its feet.
    #[must_use]
    pub fn half_height(&self) -> f32 {
        self.height * 0.5
    }

    /// The capsule.
    #[must_use]
    pub fn collider(&self) -> Collider {
        Collider::capsule(self.radius, (self.height - 2.0 * self.radius).max(0.0))
    }

    /// Whether a surface with this normal can be stood on.
    #[must_use]
    pub fn is_walkable(&self, normal: Vec3) -> bool {
        normal.y >= self.max_slope.cos()
    }

    /// The speed the command asks for, given how deep the water stands
    /// over the feet (metres, zero or less on dry ground): the walk or the
    /// sprint slowed towards the swim's speed as the water rises to the
    /// waist ([`wade_depth`](Self::wade_depth)), and the swim's speed from
    /// there on.
    #[must_use]
    pub fn speed(&self, intent: &PlayerIntent, wading: f32) -> f32 {
        let base = if intent.sprint {
            self.sprint_speed
        } else {
            self.walk_speed
        };
        let t = (wading / self.wade_depth.max(1e-3)).clamp(0.0, 1.0);
        base * (1.0 + (self.swim_factor - 1.0) * t)
    }

    /// The horizontal velocity the water gives a body whose feet stand
    /// `wading` metres under its surface: the flow's share of the body's
    /// height in it times [`current_share`](Self::current_share). A fall's
    /// water bears on the whole body from the waist
    /// ([`wade_depth`](Self::wade_depth)) and carries at least
    /// [`fall_speed`](Self::fall_speed), so a body in it is taken down and
    /// a walker in a cascade's shallows can still scramble up.
    #[must_use]
    pub fn current(&self, water: &WaterAt, wading: f32) -> Vec3 {
        let reach = if water.fall {
            self.wade_depth
        } else {
            self.height
        };
        let immersed = (wading / reach.max(1e-3)).clamp(0.0, 1.0);
        if immersed <= 0.0 {
            return Vec3::ZERO;
        }
        let mut flow = water.flow;
        if water.fall {
            let speed = flow.length();
            if speed > 1e-6 {
                flow *= speed.max(self.fall_speed) / speed;
            }
        }
        flow * (self.current_share * immersed)
    }
}

/// The water at a character's point, in render space: the surface's height
/// and the flow, from [`strata_sim::water_here`] (the sea everywhere, a
/// river or a lake where it stands higher).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterAt {
    /// The surface's render-space height.
    pub surface: f32,
    /// Downstream times the water's mean speed, horizontal.
    pub flow: Vec3,
    /// Whether it is a fall's water.
    pub fall: bool,
}

/// Gives every new [`CharacterController`] its capsule.
pub fn init_characters(
    mut commands: Commands,
    characters: Query<(Entity, &CharacterController), Without<Collider>>,
) {
    for (entity, controller) in &characters {
        commands.entity(entity).insert(controller.collider());
    }
}

/// One tick of every character: gravity, the command, move and slide, step
/// and snap, then the `WorldPosition` written. Avian's `Position` is read
/// here and written by the sync that follows, because [`MoveAndSlide`]
/// holds a read of every `Position` and a write next to it is a conflict.
pub fn step_characters(
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    source: Option<Res<WorldSourceRes>>,
    mover: MoveAndSlide,
    mut characters: Query<(
        Entity,
        &mut CharacterController,
        &Collider,
        Option<&PlayerCommand>,
        Option<&CombatMotion>,
        &Position,
        &mut WorldPosition,
        Option<&mut Riding>,
    )>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let config = MoveAndSlideConfig::default();
    for (entity, mut controller, collider, command, motion, position, mut world, mut riding) in
        &mut characters
    {
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let mut intent = command.map_or_else(PlayerIntent::default, |c| c.0);
        controller.action_velocity = motion.and_then(|motion| motion.velocity);
        if let Some(motion) = motion {
            intent.move_dir *= motion.speed_scale;
            if motion.speed_scale < 1.0 {
                intent.sprint = false;
            }
            if motion.speed_scale == 0.0 {
                intent.jump = false;
                controller.jump_buffer_left = 0.;
            }
        }
        // The water at the character's point, one sample a tick, the same
        // on a server as on the client that predicts it; taken only once
        // there is ground under it, whose colliders sampled the point's
        // water cell already, so the tick never waits on a bake. Render
        // space: the origin's height is small next to a world height, and
        // the cast is the same one the transforms make.
        let water = || {
            let s = source.as_ref()?;
            let at = origin.to_world(position.0);
            let here = water_here(s.source(), at.x(), at.z());
            #[allow(
                clippy::cast_possible_truncation,
                reason = "render-space f32 is the point"
            )]
            let surface = (f64::from(here.surface_m) - origin.0.y()) as f32;
            Some(WaterAt {
                surface,
                flow: Vec3::new(here.flow[0], 0.0, here.flow[1]),
                fall: here.fall,
            })
        };
        let next = if let Some(riding) = riding.as_deref_mut() {
            // The bike reads the command as it came: a mounted rider has no
            // action to scale it. One sample answers the water and the road.
            let command = command.map_or_else(PlayerIntent::default, |c| c.0);
            let road_rule = riding.dials.road;
            let ground = || {
                let s = source.as_ref()?;
                let at = origin.to_world(position.0);
                let sample = s.source().sample(at.x(), at.z());
                let here = water_of(&sample, s.source().sea_level());
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "render-space f32 is the point"
                )]
                let surface = (f64::from(here.surface_m) - origin.0.y()) as f32;
                Some(ride::RideGround {
                    water: Some(WaterAt {
                        surface,
                        flow: Vec3::ZERO,
                        fall: here.fall,
                    }),
                    road: sample.landform.cleared >= road_rule.cleared_min
                        || sample.landform.developed >= road_rule.developed_min,
                })
            };
            controller.ride(
                riding, &mover, collider, position.0, &command, ground, dt, &filter, &config,
            )
        } else {
            controller.step(
                &mover, collider, position.0, &intent, water, dt, &filter, &config,
            )
        };
        let Some(next) = next else {
            continue;
        };
        world.0 = origin.to_world(next);
    }
}

impl CharacterController {
    /// The position after one step of `dt` from `start`, or `None` when
    /// there is no ground at all below and the character must wait. The
    /// water at the point is asked for (`water`) only once there is
    /// ground.
    #[allow(
        clippy::too_many_arguments,
        reason = "one step, one set of inputs; a struct would only rename them"
    )]
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "collision, grounding and buffered motor response are resolved in one ordered step"
    )]
    pub fn step(
        &mut self,
        mover: &MoveAndSlide,
        collider: &Collider,
        start: Vec3,
        intent: &PlayerIntent,
        water: impl FnOnce() -> Option<WaterAt>,
        dt: f32,
        filter: &SpatialQueryFilter,
        config: &MoveAndSlideConfig,
    ) -> Option<Vec3> {
        let rotation = Quat::IDENTITY;
        let skin = config.skin_width * mover.length_unit.0;

        // Nothing to stand on anywhere below: wait for the streamer.
        mover
            .spatial_query
            .cast_ray(start, Dir3::NEG_Y, GROUND_PROBE, true, filter)?;
        let water = water();

        // Falling water holds nothing up: a body in a fall is carried down
        // its face by gravity and the flow, and cannot float up it.
        let float_y = water
            .filter(|water| !water.fall)
            .map(|water| water.surface - self.float_depth);
        let submerged = float_y.is_some_and(|float_y| start.y < float_y);
        // How deep the water stands over the feet: the speed falls with it
        // and the flow carries by it.
        let wading = water.map_or(0.0, |water| {
            (water.surface - (start.y - self.half_height())).max(0.0)
        });

        // Ground under the feet, within the snap distance, walkable.
        let ground = if self.vertical_velocity > 0.0 || submerged {
            None
        } else {
            mover
                .spatial_query
                .cast_shape(
                    collider,
                    start,
                    rotation,
                    Dir3::NEG_Y,
                    &ShapeCastConfig::from_max_distance(self.snap_distance),
                    filter,
                )
                .filter(|hit| self.is_walkable(hit.normal1))
        };
        let was_grounded = ground.is_some();
        self.ground_normal = ground.map_or(Vec3::Y, |hit| hit.normal1);
        self.swimming = submerged;
        let jump = if self.responsive {
            if intent.jump {
                self.jump_buffer_left = 0.12;
            } else {
                self.jump_buffer_left = (self.jump_buffer_left - dt).max(0.0);
            }
            if was_grounded {
                self.coyote_left = 0.1;
            } else {
                self.coyote_left = (self.coyote_left - dt).max(0.0);
            }
            self.jump_buffer_left > 0.0 && self.coyote_left > 0.0
        } else {
            intent.jump
        };
        let jumping = self.update_vertical_velocity(
            start.y,
            jump,
            float_y,
            was_grounded || (jump && self.coyote_left > 0.0),
            dt,
        );
        if jumping {
            self.jump_buffer_left = 0.0;
            self.coyote_left = 0.0;
        }

        // Horizontal motion, along the ground plane when standing on one so
        // walking downhill keeps contact.
        let speed = self.speed(intent, wading);
        let desired = intent.world_direction() * speed;
        let mut horizontal = if let Some(velocity) = self.action_velocity {
            self.horizontal_velocity = Vec3::ZERO;
            velocity
        } else if self.responsive {
            self.horizontal_velocity =
                accelerate(self.horizontal_velocity, desired, was_grounded, dt);
            self.horizontal_velocity
        } else {
            desired
        };
        if was_grounded && !jumping && horizontal != Vec3::ZERO {
            let n = self.ground_normal;
            let magnitude = horizontal.length();
            horizontal = (horizontal - n * horizontal.dot(n)).normalize_or_zero() * magnitude;
        }
        // The water's flow on top of the body's own motion, never stored
        // in it: the current stops the moment the body leaves the water.
        let current = water
            .as_ref()
            .map_or(Vec3::ZERO, |water| self.current(water, wading));
        horizontal += current;
        let velocity = horizontal + Vec3::Y * self.vertical_velocity;

        // Move and slide. Ground steeper than the limit is flattened into
        // a wall while grounded, so the character stops at it instead of
        // creeping up; airborne it keeps its true normal, so a character
        // that lands on it slides down.
        let grounded_now = was_grounded && !jumping;
        let mut landed = false;
        let mut hit_ceiling = false;
        let out = mover.move_and_slide(
            collider,
            start,
            rotation,
            velocity,
            Duration::from_secs_f32(dt),
            config,
            filter,
            |hit| {
                let n = **hit.normal;
                if self.is_walkable(n) {
                    landed = true;
                } else if n.y < -0.1 {
                    hit_ceiling = true;
                } else if grounded_now && let Ok(flat) = Dir3::new(Vec3::new(n.x, 0.0, n.z)) {
                    *hit.normal = flat;
                }
                MoveAndSlideHitResponse::Accept
            },
        );
        let mut next = out.position;

        if grounded_now {
            let wanted = horizontal * dt;
            let got = (next - start).with_y(0.0).length();
            if wanted.length() > 1e-4
                && got < wanted.length() * 0.5
                && let Some(stepped) =
                    self.step_up(mover, collider, start, wanted, got, skin, filter)
            {
                next = stepped;
                landed = true;
            }
            if !submerged && let Some(snapped) = self.snap_down(mover, collider, next, skin, filter)
            {
                next = snapped;
                landed = true;
            }
        }

        if hit_ceiling && self.vertical_velocity > 0.0 {
            self.vertical_velocity = 0.0;
        }
        if landed && self.vertical_velocity < 0.0 {
            self.vertical_velocity = 0.0;
        }
        self.grounded = landed && !submerged;
        if self.responsive && self.action_velocity.is_none() {
            // Keep a blocked axis from storing speed that would launch off a
            // wall. What the body's own motion achieved is the move less the
            // current: a swimmer held back by the flow is not blocked, and
            // must keep its stroke or it could never swim upstream. A wall
            // the current presses the body into leaves no speed backwards
            // from the body's own either, or it would push off it.
            let own = self.horizontal_velocity;
            let achieved = (next - start).with_y(0.0) / dt - current;
            if achieved.length_squared() < own.length_squared() * 0.25 {
                let heading = own.normalize_or_zero();
                self.horizontal_velocity = achieved - heading * achieved.dot(heading).min(0.0);
            }
        }
        Some(next)
    }

    /// Buoyancy, standing, a jump, or a fall. Returns whether a jump
    /// started.
    fn update_vertical_velocity(
        &mut self,
        y: f32,
        jump: bool,
        float_y: Option<f32>,
        grounded: bool,
        dt: f32,
    ) -> bool {
        if let Some(float_y) = float_y.filter(|float_y| y < *float_y) {
            // A first-order pull to the float line: no overshoot, no bob.
            self.vertical_velocity = ((float_y - y) * 4.0).clamp(-3.0, 3.0);
            false
        } else if grounded {
            if jump {
                self.vertical_velocity = self.jump_speed;
            } else {
                self.vertical_velocity = 0.0;
            }
            jump
        } else {
            self.vertical_velocity =
                (self.vertical_velocity - self.gravity * dt).max(-TERMINAL_SPEED);
            false
        }
    }

    /// A step up: when the ground blocked most of the wanted motion (only
    /// `got` metres of `wanted` were covered), try again from step height
    /// and drop back down onto what is there. The result when it gets
    /// further than the blocked move did.
    #[allow(clippy::too_many_arguments, reason = "the sweep's inputs, spelled out")]
    fn step_up(
        &self,
        mover: &MoveAndSlide,
        collider: &Collider,
        start: Vec3,
        wanted: Vec3,
        got: f32,
        skin: f32,
        filter: &SpatialQueryFilter,
    ) -> Option<Vec3> {
        let rotation = Quat::IDENTITY;
        let up = mover
            .cast_move(
                collider,
                start,
                rotation,
                Vec3::Y * self.step_height,
                skin,
                filter,
            )
            .map_or(self.step_height, |hit| hit.distance);
        let lifted = start + Vec3::Y * up;
        let forward = mover
            .cast_move(collider, lifted, rotation, wanted, skin, filter)
            .map_or(wanted, |hit| wanted.normalize_or_zero() * hit.distance);
        let stepped = lifted + forward;
        let down = mover.cast_move(collider, stepped, rotation, Vec3::NEG_Y * up, skin, filter)?;
        if !self.is_walkable(down.normal1) {
            return None;
        }
        let landed_at = stepped + Vec3::NEG_Y * down.distance;
        ((landed_at - start).with_y(0.0).length() > got).then_some(landed_at)
    }

    /// Snap down: a grounded character that did not jump follows the
    /// ground it walked off, up to a step down plus the snap distance.
    fn snap_down(
        &mut self,
        mover: &MoveAndSlide,
        collider: &Collider,
        from: Vec3,
        skin: f32,
        filter: &SpatialQueryFilter,
    ) -> Option<Vec3> {
        let hit = mover.cast_move(
            collider,
            from,
            Quat::IDENTITY,
            Vec3::NEG_Y * (self.snap_distance + self.step_height),
            skin,
            filter,
        )?;
        if !self.is_walkable(hit.normal1) {
            return None;
        }
        self.ground_normal = hit.normal1;
        Some(from - Vec3::Y * hit.distance)
    }
}

/// Bounded acceleration gives responsive starts and a short, measurable stop.
#[must_use]
pub fn accelerate(current: Vec3, desired: Vec3, grounded: bool, dt: f32) -> Vec3 {
    let braking = desired.length_squared() < current.length_squared() || current.dot(desired) < 0.0;
    let acceleration = if !grounded {
        10.0
    } else if braking {
        52.0
    } else {
        38.0
    };
    current + (desired - current).clamp_length_max(acceleration * dt.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drama measures count ground a character can walk on with this
    /// crate's steepest walkable slope (`strata_worldgen::measure`); the
    /// two must be the same angle, or the walkable share the terrain is
    /// tuned against says nothing of the character.
    #[test]
    fn the_measures_walk_on_the_characters_slope() {
        // A slope in degrees to f32 radians is what the controller stores.
        #[allow(clippy::cast_possible_truncation)]
        let measured = strata_worldgen::measure::WALKABLE_SLOPE_DEG.to_radians() as f32;
        let max_slope = CharacterController::default().max_slope;
        assert!(
            (max_slope - measured).abs() < 1e-6,
            "the controller walks up to {} deg, the measures count {} deg",
            max_slope.to_degrees(),
            strata_worldgen::measure::WALKABLE_SLOPE_DEG
        );
    }
}
