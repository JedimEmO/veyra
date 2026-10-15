//! Authored occupancy shared by tree collision and foliage rendering.

use bevy::ecs::prelude::Resource;

/// Immutable for a loaded world's lifetime; set before streaming begins.
#[derive(Resource, Clone, Debug, Default)]
pub struct VegetationExclusions(pub Vec<Footprint>);

/// The plants the placed settlements planted (their street and garden trees),
/// resolved to species and world positions: merged into each cell's
/// placement by the foliage and the physics alike, so what is drawn is what
/// collides.
#[derive(Resource, Clone, Debug, Default)]
pub struct PlantedFlora(pub std::sync::Arc<Vec<strata_flora::Planted>>);

impl PlantedFlora {
    /// The planted plants, for a task to hold.
    #[must_use]
    pub fn plants(&self) -> std::sync::Arc<Vec<strata_flora::Planted>> {
        self.0.clone()
    }
}

/// World-space horizontal rectangle, including a building's service apron.
#[derive(Clone, Debug)]
pub struct Footprint {
    /// Minimum x and z in world metres.
    pub min: [f64; 2],
    /// Maximum x and z in world metres.
    pub max: [f64; 2],
}

impl VegetationExclusions {
    /// Both rendering and physics use the same inclusive boundary.
    #[must_use]
    pub fn contains(&self, x: f64, z: f64) -> bool {
        self.0
            .iter()
            .any(|f| x >= f.min[0] && x <= f.max[0] && z >= f.min[1] && z <= f.max[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn includes_boundary_and_preserves_surrounding_land() {
        let f = VegetationExclusions(vec![Footprint {
            min: [-1500.0, -12000.0],
            max: [-1480.0, -11980.0],
        }]);
        assert!(f.contains(-1500.0, -12000.0));
        assert!(f.contains(-1490.0, -11990.0));
        assert!(!f.contains(-1500.01, -12000.0));
    }
}
