//! The simulation clock.

use serde::{Deserialize, Serialize};

/// Simulation ticks per second. 20 Hz is the rate the fixed-step schedule
/// runs at on both the server and the client (PLAN section 4.1).
pub const SIM_TICK_HZ: f64 = 20.0;

/// Where the simulation is in time, counted in ticks.
///
/// A tick count rather than a float of seconds so two machines that have
/// run the same number of steps agree bit for bit, which is the property a
/// replicated fixed-step simulation rests on. Seconds are derived, never
/// stored. `veyra-sim` wraps this in a resource and advances it once per
/// fixed step; here it is a plain value so headless tests and later the
/// server's persistence can use it without Bevy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SimTime {
    /// Ticks since the world started.
    pub tick: u64,
    /// Seconds per tick.
    pub step: f64,
}

impl SimTime {
    /// Tick zero at `hz` ticks per second.
    #[must_use]
    pub fn from_hz(hz: f64) -> Self {
        Self {
            tick: 0,
            step: 1.0 / hz,
        }
    }

    /// Seconds since the world started.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "2^53 ticks at 20 Hz is fourteen million years"
    )]
    pub fn elapsed_secs(&self) -> f64 {
        self.tick as f64 * self.step
    }

    /// One fixed step forward.
    pub fn advance(&mut self) {
        self.tick += 1;
    }

    /// Where in the day this is, in `[0, 1)`, for a day of `day_length_secs`.
    /// The atmosphere crate turns this into a sun direction.
    #[must_use]
    pub fn day_fraction(&self, day_length_secs: f64) -> f64 {
        (self.elapsed_secs() / day_length_secs).rem_euclid(1.0)
    }
}

impl Default for SimTime {
    fn default() -> Self {
        Self::from_hz(SIM_TICK_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_ticks_is_a_second() {
        let mut t = SimTime::default();
        for _ in 0..20 {
            t.advance();
        }
        assert!((t.elapsed_secs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn day_fraction_wraps() {
        let mut t = SimTime::from_hz(1.0);
        t.tick = 90;
        assert!((t.day_fraction(60.0) - 0.5).abs() < 1e-12);
    }
}
