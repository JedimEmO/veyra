//! Finding somewhere to stand.
//!
//! A seed's origin is as likely to be sea as land, so the app asks the world
//! for the nearest dry point instead of guessing. Pure and deterministic,
//! like the rest of the crate: the server picks spawn points with the same
//! function.

use crate::WorldSource;

/// The nearest point to `(x, z)` whose height is at least `margin` metres
/// above sea level, searched on a square spiral with `step` metres between
/// probes out to `radius`. `None` when everything within the radius is sea.
///
/// A spiral rather than a scan so the answer is the closest such probe, and
/// the probe spacing keeps the cost bounded: 20 km at 250 m is 6400 samples,
/// a few milliseconds.
#[must_use]
pub fn find_land_near(
    source: &dyn WorldSource,
    x: f64,
    z: f64,
    margin: f32,
    step: f64,
    radius: f64,
) -> Option<(f64, f64)> {
    let sea = source.sea_level();
    let dry = |px: f64, pz: f64| source.sample(px, pz).height >= sea + margin;
    if dry(x, z) {
        return Some((x, z));
    }
    // Rings of the spiral, each a square of side 2 * ring * step.
    let rings = (radius / step).floor().max(0.0);
    // Rings are counted, not measured; the value is small and positive.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings = rings as i64;
    for ring in 1..=rings {
        let best = square_ring(ring)
            .map(|(i, j)| {
                // Offsets are small integers; exact in f64.
                #[allow(clippy::cast_precision_loss)]
                let (dx, dz) = (i as f64 * step, j as f64 * step);
                (x + dx, z + dz, (dx * dx + dz * dz).sqrt())
            })
            .filter(|(px, pz, _)| dry(*px, *pz))
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((px, pz, _)) = best {
            return Some((px, pz));
        }
    }
    None
}

/// The integer offsets on the boundary of the square `[-ring, ring]^2`.
fn square_ring(ring: i64) -> impl Iterator<Item = (i64, i64)> {
    (-ring..=ring).flat_map(move |i| {
        (-ring..=ring).filter_map(move |j| (i.abs() == ring || j.abs() == ring).then_some((i, j)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sample;
    use strata_registry::BiomeId;

    /// Land east of `x = 1000`, sea everywhere else.
    struct Shore;

    impl WorldSource for Shore {
        fn sample(&self, x: f64, _z: f64) -> Sample {
            let height = if x >= 1_000.0 { 50.0 } else { -50.0 };
            Sample {
                height,
                biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.5,
                hydration: 0.5,
                cover: 0.5,
                landform: crate::Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    #[test]
    fn a_dry_start_is_returned_as_is() {
        assert_eq!(
            find_land_near(&Shore, 2_000.0, 5.0, 1.0, 100.0, 5_000.0),
            Some((2_000.0, 5.0))
        );
    }

    #[test]
    fn the_nearest_probe_on_land_wins() {
        let found =
            find_land_near(&Shore, 0.0, 0.0, 1.0, 100.0, 5_000.0).expect("land within 5 km");
        assert_eq!(found, (1_000.0, 0.0));
    }

    #[test]
    fn all_sea_is_none() {
        assert_eq!(
            find_land_near(&Shore, -50_000.0, 0.0, 1.0, 100.0, 2_000.0),
            None
        );
    }

    #[test]
    fn rings_have_the_right_size() {
        assert_eq!(square_ring(1).count(), 8);
        assert_eq!(square_ring(3).count(), 24);
    }
}
