//! A scripted flight: the camera follows a path on its own.
//!
//! For verification and benchmarking rather than play: the same path at the
//! same speed every run, so two builds can be compared frame for frame and
//! a screenshot at a given frame shows the same place. The path is a
//! straight run along a heading with a slow weave, at a fixed height above
//! the ground, which crosses region borders on both axes and every LOD band
//! without ever flying into a hill.

use bevy::ecs::prelude::*;
use bevy::math::DVec3;
use bevy::time::Time;
use strata_sim::{WorldPosition, WorldSourceRes};

use crate::flycam::FlyCam;

/// The path an entity follows. Put it on the flycam (or on any entity with
/// a `WorldPosition`; a `FlyCam` next to it also gets its yaw and pitch
/// steered).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FlightPath {
    /// Ground speed in metres per second.
    pub speed: f64,
    /// Base heading, radians about `y`; zero flies towards `-z` like a yaw
    /// of zero on the flycam.
    pub heading: f64,
    /// Amplitude of the weave in radians, added to the heading as a sine
    /// of time.
    pub weave: f64,
    /// Period of the weave in seconds.
    pub weave_period: f64,
    /// Height above the ground (or the sea) to hold.
    pub altitude: f64,
    /// Radians below the horizon the camera looks.
    pub pitch: f64,
    /// Seconds flown so far.
    pub elapsed: f64,
}

impl Default for FlightPath {
    fn default() -> Self {
        Self {
            speed: 200.0,
            heading: 0.0,
            weave: 0.6,
            weave_period: 40.0,
            altitude: 120.0,
            pitch: 0.22,
            elapsed: 0.0,
        }
    }
}

impl FlightPath {
    /// The heading at `elapsed` seconds.
    #[must_use]
    pub fn heading_at(&self, elapsed: f64) -> f64 {
        if self.weave_period <= 0.0 {
            return self.heading;
        }
        self.heading + self.weave * (elapsed / self.weave_period * core::f64::consts::TAU).sin()
    }

    /// The unit horizontal direction of travel at `elapsed` seconds. Yaw
    /// zero is `-z`, positive yaw turns towards `-x`, the flycam's frame.
    #[must_use]
    pub fn direction_at(&self, elapsed: f64) -> DVec3 {
        let yaw = self.heading_at(elapsed);
        DVec3::new(-yaw.sin(), 0.0, -yaw.cos())
    }
}

/// While this resource exists and is held, scripted motion waits at its
/// start: the flight path does not advance and the scripted walkers stand.
///
/// A benchmark of a moving fixture inserts it held and releases it once the
/// world around the start has streamed in (`stats::BenchRecorder`). A
/// fixture that moves from the first frame measures the first fill around
/// a new camera, about 35 s of two terrain uploads a frame, and a walker
/// that outruns its colliders; neither is what moving costs. Without the
/// resource nothing waits, which is every run but a benchmark's.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MotionHold {
    released: bool,
}

impl MotionHold {
    /// Whether scripted motion must still wait.
    #[must_use]
    pub fn held(&self) -> bool {
        !self.released
    }

    /// Lets scripted motion start; it stays released.
    pub fn release(&mut self) {
        self.released = true;
    }
}

/// Whether scripted motion must wait: a [`MotionHold`] exists and is held.
#[must_use]
pub fn motion_held(hold: Option<&MotionHold>) -> bool {
    hold.is_some_and(MotionHold::held)
}

/// Moves every [`FlightPath`] entity along its path. Runs after the manual
/// flycam systems so the script wins; the ground height is one sample per
/// frame, smoothed so a ridge below does not throw the camera about. Under
/// a held [`MotionHold`] the camera stays at its start, looking along the
/// path's first heading, so what streams in is what the flight sees first.
pub fn fly_path(
    time: Res<Time>,
    source: Option<Res<WorldSourceRes>>,
    hold: Option<Res<MotionHold>>,
    mut flights: Query<(&mut FlightPath, &mut WorldPosition, Option<&mut FlyCam>)>,
) {
    let dt = if motion_held(hold.as_deref()) {
        0.0
    } else {
        f64::from(time.delta_secs())
    };
    for (mut flight, mut position, cam) in &mut flights {
        let dir = flight.direction_at(flight.elapsed);
        let mut next = position.0 + dir * flight.speed * dt;
        if let Some(source) = &source {
            let ground = f64::from(
                source
                    .source()
                    .sample(next.x(), next.z())
                    .height
                    .max(source.source().sea_level()),
            );
            let target = ground + flight.altitude;
            // A first-order lag with a one-second time constant: fast
            // enough to clear a hill at 200 m/s, slow enough not to bob.
            let k = (dt / 1.0).min(1.0);
            let y = next.y() + (target - next.y()) * k;
            next = next.with_y(y);
        }
        position.0 = next;
        flight.elapsed += dt;
        if let Some(mut cam) = cam {
            // Yaw and pitch are small angles; f32 is what the camera holds.
            #[allow(clippy::cast_possible_truncation)]
            {
                cam.yaw = flight.heading_at(flight.elapsed) as f32;
                cam.pitch = -(flight.pitch as f32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_zero_flies_north_along_negative_z() {
        let path = FlightPath {
            weave: 0.0,
            ..Default::default()
        };
        let d = path.direction_at(3.0);
        assert!((d - DVec3::NEG_Z).length() < 1e-12, "{d}");
    }

    #[test]
    fn the_weave_returns_to_the_base_heading_each_period() {
        let path = FlightPath::default();
        let h0 = path.heading_at(0.0);
        let h1 = path.heading_at(path.weave_period);
        assert!((h0 - h1).abs() < 1e-9);
        assert!(
            (path.heading_at(path.weave_period * 0.25) - (path.heading + path.weave)).abs() < 1e-9
        );
    }
}
