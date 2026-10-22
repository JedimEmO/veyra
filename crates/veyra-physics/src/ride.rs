//! The ridden motor (ticket 0009, step 2): the same capsule, the same move
//! and slide, step up and snap down as the walk, with a different motor in
//! front of them.
//!
//! The numbers are all [`RideDials`], copied onto the rider at the mount, so
//! nothing here reads a registry and a server steps a bike as a client
//! predicts it. The pieces that decide a figure ([`turn_rate`],
//! [`slope_share`], [`water_top`], [`steer`], [`drive`], [`is_crash`]) are
//! pure functions of the dials, which is how they are tested without a
//! ground.
//!
//! The hover is drawn, not simulated: the capsule stays on the ground.
//!
//! # Where the road is read
//!
//! One `Sample` a tick answers both the water and the road: the sample's
//! `cleared` (the carriageway and verge) or `developed` (a settlement's
//! ground), against the dials. That is the cheapest correct read: the
//! sample is taken for the water already, and `cleared` is the very field
//! the grading writes, so a bridge deck and a verge count as road where
//! `nearest_road` would need a second search over the network.
//!
//! # Reversing
//!
//! A wish within 100 degrees of the heading is throttle; a wish behind it
//! is the brake, then the bike turns on the spot at the slow rate until the
//! wish is ahead. It reverses only when S alone is held (no sideways) with
//! the camera within 100 degrees of the heading, i.e. the camera behind the
//! bike, and then does not turn.

use core::f32::consts::{PI, TAU};
use core::time::Duration;

use avian3d::prelude::{
    Collider, MoveAndSlide, MoveAndSlideConfig, MoveAndSlideHitResponse, Position, ShapeCastConfig,
    SpatialQueryFilter,
};
use bevy::ecs::prelude::*;
use bevy::math::{Dir3, Quat, Vec3};
use bevy::time::{Fixed, Time};
use strata_app::water::water_of;
use strata_app::{
    CombatMotion, PlayerCommand, PlayerIntent, RenderOrigin, WorldPosition, WorldSourceRes,
};
use strata_physics::character::{ExternalMotor, GROUND_PROBE, TERMINAL_SPEED};
use strata_physics::{CharacterController, WaterAt};
use veyra_rules::ride::{RideDials, RideEnd, Riding};

/// Puts [`ExternalMotor`] on a character the moment it mounts, so the walk
/// step leaves it to [`step_riders`] in the same tick the mount is made.
pub fn mark_rider(add: On<Add, Riding>, mut commands: Commands) {
    commands.entity(add.entity).insert(ExternalMotor);
}

/// Hands a dismounted character back to the walk step.
pub fn release_rider(remove: On<Remove, Riding>, mut commands: Commands) {
    commands.entity(remove.entity).try_remove::<ExternalMotor>();
}

/// One tick of every rider: the ridden motor in place of the walk step, with
/// the same position write the walk step makes.
pub fn step_riders(
    time: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    source: Option<Res<WorldSourceRes>>,
    mover: MoveAndSlide,
    mut riders: Query<(
        Entity,
        &mut CharacterController,
        &Collider,
        Option<&PlayerCommand>,
        Option<&CombatMotion>,
        &Position,
        &mut WorldPosition,
        &mut Riding,
    )>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let config = MoveAndSlideConfig::default();
    for (entity, mut controller, collider, command, motion, position, mut world, mut riding) in
        &mut riders
    {
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        controller.action_velocity = motion.and_then(|motion| motion.velocity);
        // A body moved by a hand other than the step is airborne until the
        // step says otherwise, as for the walker.
        if controller.rested > 0 && controller.rest_at != Some(*world) {
            controller.rested = 0;
            controller.grounded = false;
        }
        controller.rested = 0;
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
            Some(RideGround {
                water: Some(WaterAt {
                    surface,
                    flow: Vec3::ZERO,
                    fall: here.fall,
                }),
                road: sample.landform.cleared >= road_rule.cleared_min
                    || sample.landform.developed >= road_rule.developed_min,
            })
        };
        let Some(next) = ride(
            &mut controller,
            &mut riding,
            &mover,
            collider,
            position.0,
            &command,
            ground,
            dt,
            &filter,
            &config,
        ) else {
            continue;
        };
        world.0 = origin.to_world(next);
    }
}

/// What the ground under the bike says, from the one sample a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideGround {
    /// The water at the point, if the world is asked.
    pub water: Option<WaterAt>,
    /// Whether the point is road by the dials.
    pub road: bool,
}

/// What the rider asks of the engine this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Throttle {
    /// Towards the top speed.
    Go,
    /// Towards a stop, hard.
    Brake,
    /// Backwards.
    Reverse,
    /// Nothing asked: drag.
    Coast,
}

/// `angle` wrapped to `(-PI, PI]`.
#[must_use]
pub fn wrap(angle: f32) -> f32 {
    let a = angle.rem_euclid(TAU);
    if a > PI { a - TAU } else { a }
}

/// The turn rate at `speed`, degrees a second: the slow rate at a crawl,
/// the fast one at the road's top speed, straight between.
#[must_use]
pub fn turn_rate(dials: &RideDials, speed: f32) -> f32 {
    let t = (speed.abs() / dials.speed.road).clamp(0.0, 1.0);
    dials.turn.slow + (dials.turn.fast - dials.turn.slow) * t
}

/// The share of the top speed a climb of `deg` degrees keeps: all of it up
/// to the full angle, none from the stop angle, straight between. Downhill
/// (negative) costs nothing.
#[must_use]
pub fn slope_share(dials: &RideDials, deg: f32) -> f32 {
    let (full, stop) = (dials.slope.full_deg, dials.slope.stop_deg);
    ((stop - deg) / (stop - full)).clamp(0.0, 1.0)
}

/// How steeply the ground with `normal` rises along the horizontal `dir`,
/// degrees, positive uphill.
#[must_use]
pub fn climb_deg(normal: Vec3, dir: Vec3) -> f32 {
    if normal.y <= 1e-3 {
        return 90.0;
    }
    (-(normal.x * dir.x + normal.z * dir.z) / normal.y)
        .atan()
        .to_degrees()
}

/// The top speed in water `wading` metres over the ground, given the speed
/// it would have dry: all of it up to the ride depth, falling straight to the
/// crawl at the stall depth, and `None` (thrown) beyond.
#[must_use]
pub fn water_top(dials: &RideDials, wading: f32, dry: f32) -> Option<f32> {
    let w = dials.water;
    if wading > w.stall_m {
        return None;
    }
    if wading <= w.ride_m {
        return Some(dry);
    }
    let t = (wading - w.ride_m) / (w.stall_m - w.ride_m);
    Some((dry + (w.crawl - dry) * t).min(dry))
}

/// Turns `heading` towards the wish at the rate for `speed`, and says what
/// the engine is asked.
#[must_use]
pub fn steer(
    dials: &RideDials,
    heading: f32,
    speed: f32,
    intent: &PlayerIntent,
    dt: f32,
) -> (f32, Throttle) {
    let wish = intent.world_direction();
    // The vehicle's file: how far from the heading a wish is still the
    // throttle, and the stick's dead zone.
    let (arc, dead) = (
        dials.turn.throttle_arc_deg.to_radians(),
        dials.turn.dead_zone,
    );
    if wish.length() < dead {
        return (heading, Throttle::Coast);
    }
    let behind_camera = wrap(intent.yaw - heading).abs() < arc;
    if intent.move_dir.y < -dead && intent.move_dir.x.abs() < dead && behind_camera {
        return (heading, Throttle::Reverse);
    }
    let wanted = (-wish.x).atan2(-wish.z);
    let diff = wrap(wanted - heading);
    let rate = turn_rate(dials, speed).to_radians() * dt;
    let throttle = if diff.abs() <= arc {
        Throttle::Go
    } else {
        Throttle::Brake
    };
    (wrap(heading + diff.clamp(-rate, rate)), throttle)
}

/// The speed after a tick of `throttle`, with `top` the forward top speed
/// the ground, slope and water allow.
#[must_use]
pub fn drive(dials: &RideDials, speed: f32, throttle: Throttle, top: f32, dt: f32) -> f32 {
    let target = match throttle {
        Throttle::Go => top,
        Throttle::Brake | Throttle::Coast => 0.0,
        Throttle::Reverse => -dials.speed.reverse.min(top.max(0.0)),
    };
    let rate = if throttle == Throttle::Coast {
        dials.drag
    } else if speed.abs() < target.abs() && speed * target >= 0.0 {
        dials.accel
    } else {
        dials.brake
    };
    speed + (target - speed).clamp(-rate * dt, rate * dt)
}

/// Whether a tick that moved the bike at `achieved` metres a second when it
/// asked for `before`, against something not walkable, was a crash.
#[must_use]
pub fn is_crash(dials: &RideDials, before: f32, achieved: f32, hit_wall: bool) -> bool {
    hit_wall && before > dials.crash.speed_min && achieved < before * (1.0 - dials.crash.lost_share)
}

/// The position after one ridden step of `dt` from `start`, or `None`
/// when there is no ground at all below and the bike must wait. `ground`
/// is asked once, and only when there is ground. The rider's heading
/// and speed are advanced in `riding`, and a crash or deep water is
/// reported in [`Riding::ended`] for the sim to throw them.
#[allow(
    clippy::too_many_arguments,
    reason = "one step, one set of inputs; a struct would only rename them"
)]
#[allow(
    clippy::too_many_lines,
    reason = "the motor, the slide, the step up and the crash are one ordered step"
)]
#[must_use]
pub fn ride(
    controller: &mut CharacterController,
    riding: &mut Riding,
    mover: &MoveAndSlide,
    collider: &Collider,
    start: Vec3,
    intent: &PlayerIntent,
    ground: impl FnOnce() -> Option<RideGround>,
    dt: f32,
    filter: &SpatialQueryFilter,
    config: &MoveAndSlideConfig,
) -> Option<Vec3> {
    // The bike's wall is its own stop angle, not the walker's: ground
    // steeper than that is flattened to a wall by the slide, the step up
    // and the snap down alike, which all ask `is_walkable`.
    let walker = controller.max_slope;
    controller.max_slope = walker.min(riding.dials.slope.stop_deg.to_radians());
    let next = ride_step(
        controller, riding, mover, collider, start, intent, ground, dt, filter, config,
    );
    controller.max_slope = walker;
    next
}

#[allow(
    clippy::too_many_arguments,
    reason = "one step, one set of inputs; a struct would only rename them"
)]
#[allow(
    clippy::too_many_lines,
    reason = "the motor, the slide, the step up and the crash are one ordered step"
)]
fn ride_step(
    controller: &mut CharacterController,
    riding: &mut Riding,
    mover: &MoveAndSlide,
    collider: &Collider,
    start: Vec3,
    intent: &PlayerIntent,
    ground: impl FnOnce() -> Option<RideGround>,
    dt: f32,
    filter: &SpatialQueryFilter,
    config: &MoveAndSlideConfig,
) -> Option<Vec3> {
    let rotation = Quat::IDENTITY;
    let skin = config.skin_width * mover.length_unit.0;
    let dials = riding.dials;

    mover
        .spatial_query
        .cast_ray(start, Dir3::NEG_Y, GROUND_PROBE, true, filter)?;
    let here = ground().unwrap_or(RideGround {
        water: None,
        road: false,
    });
    let wading = here.water.map_or(0.0, |water| {
        (water.surface - (start.y - controller.half_height())).max(0.0)
    });
    let floor = if here.road {
        dials.speed.road
    } else {
        dials.speed.ground
    };
    let Some(dry_cap) = water_top(&dials, wading, floor) else {
        riding.ended = Some(RideEnd::Water);
        riding.speed = 0.0;
        riding.velocity = Vec3::ZERO;
        return Some(start);
    };

    let under = if controller.vertical_velocity > 0.0 {
        None
    } else {
        mover
            .spatial_query
            .cast_shape(
                collider,
                start,
                rotation,
                Dir3::NEG_Y,
                &ShapeCastConfig::from_max_distance(controller.snap_distance),
                filter,
            )
            .filter(|hit| controller.is_walkable(hit.normal1))
    };
    let was_grounded = under.is_some();
    controller.ground_normal = under.map_or(Vec3::Y, |hit| hit.normal1);
    controller.swimming = false;
    controller.horizontal_velocity = Vec3::ZERO;
    controller.jump_buffer_left = 0.0;
    controller.coyote_left = 0.0;
    controller.vertical_velocity = if was_grounded {
        0.0
    } else {
        (controller.vertical_velocity - controller.gravity * dt).max(-TERMINAL_SPEED)
    };

    let (heading, throttle) = steer(&dials, riding.heading, riding.speed, intent, dt);
    riding.heading = heading;
    let facing = PlayerIntent::forward(heading);
    let along = if throttle == Throttle::Reverse {
        -facing
    } else {
        facing
    };
    let climb = if was_grounded {
        climb_deg(controller.ground_normal, along)
    } else {
        0.0
    };
    // In the air nothing pushes: the engine cannot raise the speed.
    let top = if was_grounded {
        dry_cap * slope_share(&dials, climb)
    } else {
        riding.speed.abs()
    };
    riding.speed = drive(&dials, riding.speed, throttle, top, dt);

    // The velocity swings to the heading at the grip, so a turn slides.
    let grip = (dials.turn.grip * dt).min(1.0);
    let wanted = facing * riding.speed;
    let carried = riding.velocity + (wanted - riding.velocity) * grip;
    let mut horizontal = carried;
    if was_grounded && horizontal != Vec3::ZERO {
        let n = controller.ground_normal;
        let magnitude = horizontal.length();
        horizontal = (horizontal - n * horizontal.dot(n)).normalize_or_zero() * magnitude;
    }
    let velocity = horizontal + Vec3::Y * controller.vertical_velocity;

    let mut landed = false;
    let mut hit_wall = false;
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
            if controller.is_walkable(n) {
                landed = true;
            } else {
                hit_wall = true;
                if was_grounded
                    && n.y >= -0.1
                    && let Ok(flat) = Dir3::new(Vec3::new(n.x, 0.0, n.z))
                {
                    *hit.normal = flat;
                }
            }
            MoveAndSlideHitResponse::Accept
        },
    );
    let mut next = out.position;
    if was_grounded {
        let asked = horizontal * dt;
        let got = (next - start).with_y(0.0).length();
        if asked.length() > 1e-4
            && got < asked.length() * 0.5
            && let Some(stepped) =
                controller.step_up(mover, collider, start, asked, got, skin, filter)
        {
            next = stepped;
            landed = true;
            // A kerb ridden over is no wall.
            hit_wall = false;
        }
        if let Some(snapped) = controller.snap_down(mover, collider, next, skin, filter) {
            next = snapped;
            landed = true;
        }
    }
    if landed && controller.vertical_velocity < 0.0 {
        controller.vertical_velocity = 0.0;
    }
    controller.grounded = landed;

    // The loss is judged on the slide's clipped velocity, which does not
    // depend on where in the tick the wall was met.
    let achieved = out.projected_velocity.with_y(0.0);
    if is_crash(&dials, carried.length(), achieved.length(), hit_wall) {
        riding.ended = Some(RideEnd::Crash(carried.length()));
        riding.speed = 0.0;
        riding.velocity = Vec3::ZERO;
    } else if hit_wall {
        // A scrape keeps what the slide kept and no more.
        let kept = achieved.dot(facing);
        riding.speed = if riding.speed >= 0.0 {
            riding.speed.min(kept.max(0.0))
        } else {
            riding.speed.max(kept.min(0.0))
        };
        riding.velocity = achieved;
    } else {
        riding.velocity = carried;
    }
    Some(next)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use bevy::math::Vec2;
    use strata_registry::vehicle::{
        CallRule, Crash, RoadRule, Slope, Speeds, Turn, VehicleDef, Water,
    };

    fn dials() -> RideDials {
        RideDials::of(&VehicleDef {
            version: 1,
            id: strata_registry::Namespaced::parse("veyra:vehicle.test").unwrap(),
            name: "test".to_owned(),
            speed: Speeds {
                road: 30.0,
                ground: 18.0,
                reverse: 4.0,
            },
            accel: 9.0,
            brake: 16.0,
            drag: 3.0,
            turn: Turn {
                slow: 150.0,
                fast: 55.0,
                grip: 6.0,
                throttle_arc_deg: 100.0,
                dead_zone: 0.1,
            },
            slope: Slope {
                full_deg: 14.0,
                stop_deg: 36.0,
            },
            water: Water {
                ride_m: 0.5,
                stall_m: 1.1,
                crawl: 3.0,
            },
            road: RoadRule {
                cleared_min: 0.5,
                developed_min: 0.5,
            },
            crash: Crash {
                speed_min: 12.0,
                lost_share: 0.6,
            },
            call: CallRule {
                channel_s: 1.5,
                clear_m: 40.0,
                feet_m: 0.85,
            },
            view: strata_registry::vehicle::View {
                hover_m: 0.9,
                camera_distance: 9.0,
                camera_eye: 1.6,
                seat: [0.0, 0.55, -0.1],
                lean_deg: 20.0,
                pitch_max: 0.45,
                bob_m: 0.03,
                bob_period_s: 2.6,
                ease_s: 0.5,
                follow: 8.0,
            },
            lines: strata_registry::vehicle::Lines {
                speed: [0.9, 1.1],
                handling: [0.85, 1.15],
            },
            cargo: strata_registry::vehicle::Cargo {
                columns: 6,
                rows: 3,
            },
            wear: strata_registry::vehicle::VehicleWear {
                crash: 120,
                speed_cap: 3.0,
                worst_speed: 0.6,
            },
        })
    }

    #[test]
    fn the_turn_rate_runs_from_slow_at_a_crawl_to_fast_at_the_road_speed() {
        let d = dials();
        assert!((turn_rate(&d, 0.0) - 150.0).abs() < 1e-4);
        assert!((turn_rate(&d, 30.0) - 55.0).abs() < 1e-4);
        assert!((turn_rate(&d, -30.0) - 55.0).abs() < 1e-4);
        assert!((turn_rate(&d, 15.0) - 102.5).abs() < 1e-4);
    }

    #[test]
    fn a_climb_costs_nothing_to_the_full_angle_and_everything_at_the_stop_angle() {
        let d = dials();
        assert!((slope_share(&d, 14.0) - 1.0).abs() < 1e-6);
        assert!((slope_share(&d, -20.0) - 1.0).abs() < 1e-6);
        assert!(slope_share(&d, 36.0).abs() < 1e-6);
        assert!(slope_share(&d, 50.0).abs() < 1e-6);
        assert!((slope_share(&d, 25.0) - 0.5).abs() < 1e-6);
        // A 14 degree ramp rising along +x, read along +x and along -x.
        let t = 14.0_f32.to_radians();
        let normal = Vec3::new(-t.sin(), t.cos(), 0.0);
        assert!((climb_deg(normal, Vec3::X) - 14.0).abs() < 1e-3);
        assert!((climb_deg(normal, Vec3::NEG_X) + 14.0).abs() < 1e-3);
    }

    #[test]
    fn water_is_ridden_to_the_ride_depth_slows_to_the_crawl_and_throws_past_the_stall() {
        let d = dials();
        assert_eq!(water_top(&d, 0.0, 18.0), Some(18.0));
        assert_eq!(water_top(&d, 0.5, 18.0), Some(18.0));
        let mid = water_top(&d, 0.8, 18.0).unwrap();
        assert!((mid - 10.5).abs() < 1e-4, "{mid}");
        assert!((water_top(&d, 1.1, 18.0).unwrap() - 3.0).abs() < 1e-4);
        assert_eq!(water_top(&d, 1.2, 18.0), None);
    }

    #[test]
    fn a_wish_ahead_is_throttle_behind_is_brake_and_s_alone_reverses() {
        let d = dials();
        let ahead = PlayerIntent {
            move_dir: Vec2::Y,
            yaw: 0.0,
            ..Default::default()
        };
        assert_eq!(steer(&d, 0.0, 10.0, &ahead, 0.05).1, Throttle::Go);
        let behind = PlayerIntent {
            move_dir: Vec2::Y,
            yaw: PI,
            ..Default::default()
        };
        let (heading, throttle) = steer(&d, 0.0, 10.0, &behind, 0.05);
        assert_eq!(throttle, Throttle::Brake);
        assert!(heading.abs() > 0.0, "it turns while it brakes");
        let back = PlayerIntent {
            move_dir: -Vec2::Y,
            yaw: 0.0,
            ..Default::default()
        };
        assert_eq!(steer(&d, 0.0, 1.0, &back, 0.05), (0.0, Throttle::Reverse));
        let idle = PlayerIntent::default();
        assert_eq!(steer(&d, 0.0, 1.0, &idle, 0.05).1, Throttle::Coast);
    }

    #[test]
    fn the_speed_gathers_brakes_and_coasts_by_the_dials() {
        let d = dials();
        assert!((drive(&d, 0.0, Throttle::Go, 18.0, 0.5) - 4.5).abs() < 1e-5);
        assert!((drive(&d, 18.0, Throttle::Go, 18.0, 0.5) - 18.0).abs() < 1e-5);
        assert!((drive(&d, 10.0, Throttle::Brake, 18.0, 0.25) - 6.0).abs() < 1e-5);
        assert!((drive(&d, 10.0, Throttle::Coast, 18.0, 0.5) - 8.5).abs() < 1e-5);
        assert!((drive(&d, 0.0, Throttle::Reverse, 18.0, 5.0) + 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_crash_is_a_wall_that_takes_most_of_a_fast_speed() {
        let d = dials();
        assert!(is_crash(&d, 20.0, 2.0, true));
        assert!(!is_crash(&d, 8.0, 0.0, true), "too slow to crash");
        assert!(!is_crash(&d, 20.0, 15.0, true), "a glance");
        assert!(!is_crash(&d, 20.0, 0.0, false), "no wall");
    }
}
