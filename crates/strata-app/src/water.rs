//! The water a body stands in (ticket 0005, step 3), below the render layer
//! so a server moves its characters through rivers exactly as a client
//! predicts them (ADR 0004).
//!
//! The sea is one level everywhere, as it always was; a river or a lake is
//! the sample's own record where its water stands over the ground
//! ([`Sample::standing_water`]): within a river's banks, under a lake's
//! level. Of the two the higher surface is the one a body is in, so a mouth
//! hands a swimmer from the river to the sea where the river's surface
//! comes down to the sea's.

use strata_worldgen::hydrology::WaterKind;
use strata_worldgen::{Sample, WorldSource};

pub use strata_types::WaterHere;

/// The water at `(x, z)` of `source`: the sea, or a river or a lake whose
/// water stands higher there. One sample, so a character pays one a tick.
#[must_use]
pub fn water_here(source: &dyn WorldSource, x: f64, z: f64) -> WaterHere {
    water_of(&source.sample(x, z), source.sea_level())
}

/// [`water_here`] from a sample already taken.
#[must_use]
pub fn water_of(sample: &Sample, sea_level: f32) -> WaterHere {
    let sea = WaterHere {
        surface_m: sea_level,
        ground_m: sample.height,
        flow: [0.0, 0.0],
        fall: false,
    };
    match sample.standing_water() {
        Some(water) if water.surface_m > sea_level => WaterHere {
            surface_m: water.surface_m,
            ground_m: sample.height,
            flow: water.flow,
            fall: water.kind == WaterKind::Fall,
        },
        _ => sea,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::BiomeId;
    use strata_worldgen::{Landform, SampleWater};

    fn sample(height: f32, water: Option<SampleWater>) -> Sample {
        Sample {
            height,
            biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
            provinces: Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.5,
            hydration: 0.5,
            cover: 0.5,
            landform: Landform::default(),
            water,
        }
    }

    fn river(signed_distance_m: f32) -> SampleWater {
        SampleWater {
            surface_m: 10.0,
            bed_m: 9.4,
            width_m: 6.0,
            signed_distance_m,
            flow: [0.6, 0.0],
            discharge_m3s: 2.0,
            order: 2,
            kind: WaterKind::Channel,
            cut_dm: 0,
        }
    }

    /// In the channel the river's surface and flow; on a floodplain under
    /// its level but past its banks, and on the bank over it, the sea's.
    #[test]
    fn a_river_stands_only_within_its_banks() {
        let wet = water_of(&sample(9.5, Some(river(1.0))), 0.0);
        assert!((wet.surface_m - 10.0).abs() < 1e-6 && (wet.flow[0] - 0.6).abs() < 1e-6);
        assert!((wet.depth_m() - 0.5).abs() < 1e-6);
        let floodplain = water_of(&sample(9.0, Some(river(40.0))), 0.0);
        assert!(floodplain.surface_m.abs() < 1e-6 && floodplain.flow[0].abs() < 1e-6);
        let bank = water_of(&sample(10.4, Some(river(3.5))), 0.0);
        assert!(bank.depth_m() < 0.0);
    }

    /// Where a river's surface is down at the sea's, the sea is the water.
    #[test]
    fn a_mouth_hands_over_to_the_sea() {
        let mut at_sea = river(0.0);
        at_sea.surface_m = 0.0;
        at_sea.bed_m = -0.6;
        let here = water_of(&sample(-0.5, Some(at_sea)), 0.0);
        assert!(here.flow[0].abs() < 1e-6 && here.surface_m.abs() < 1e-6);
    }
}
