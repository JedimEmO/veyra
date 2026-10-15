use ashlar::{GroundRect, GroundingPolicy, GroundingSpec, Pose, Socket};
use glam::DQuat;

/// Explicit outpost floor/support/entrance contract, matching the authored slab.
pub fn grounding_spec() -> GroundingSpec {
    GroundingSpec {
        footprint: GroundRect::new([-0.325, -0.325], [12.325, 8.325]),
        floor: -0.02,
        slab_bottom: -0.32,
        entrance: Socket::new(
            "entrance",
            Pose::at([6.0, 0.1, -0.48]).rotated(DQuat::from_rotation_y(std::f64::consts::PI)),
        ),
    }
}
/// Initial outpost placement limits, kept in content rather than solver defaults.
pub fn grounding_policy() -> GroundingPolicy {
    GroundingPolicy {
        clearance: 0.2,
        embed: 0.35,
        max_depth: 4.0,
        support_width: 0.32,
        max_span: 2.0,
        landing_length: 1.5,
        ramp_width: 2.4,
        max_grade: 0.28,
        max_ramp_length: 18.0,
        search_step: 0.5,
        vegetation_margin: 0.75,
        max_toe_step: 0.2,
    }
}
