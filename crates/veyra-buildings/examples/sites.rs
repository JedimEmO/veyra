//! Offline site search; never changes the world.
use ashlar::{TerrainPatch, fit_ground};
use strata_worldgen::{ProceduralWorld, WorldSource};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registries = strata_registry::load_from_dir(std::path::Path::new("assets"))?;
    let source = ProceduralWorld::new(strata_core::Seed(42), &registries);
    for z in (-12200..=-11800).step_by(40) {
        for x in (-1700..=-1300).step_by(40) {
            let at = [f64::from(x), f64::from(z)];
            let origin = [at[0] - 40.0, at[1] - 40.0];
            let mut heights = Vec::new();
            for iz in 0..81 {
                for ix in 0..81 {
                    heights.push(f64::from(
                        source
                            .sample(origin[0] + f64::from(ix), origin[1] + f64::from(iz))
                            .height,
                    ));
                }
            }
            let patch = TerrainPatch::new(origin, 1.0, [81, 81], heights)?;
            for yaw in [
                0.0,
                0.65,
                std::f64::consts::FRAC_PI_2,
                2.4,
                std::f64::consts::PI,
                4.1,
                3.0 * std::f64::consts::FRAC_PI_2,
                5.5,
            ] {
                if let Ok(fit) = fit_ground(
                    &veyra_buildings::grounding_spec(),
                    veyra_buildings::grounding_policy(),
                    &patch,
                    at,
                    yaw,
                ) && fit.terrain_range[0] > 5.0
                {
                    println!(
                        "{x} {z} {yaw} relief={:.3} ramp={:.1} grade={:.3} floor={:.2}",
                        fit.terrain_range[1] - fit.terrain_range[0],
                        fit.ramp_length,
                        fit.ramp_grade,
                        fit.pose.translation.y
                    );
                    break;
                }
            }
        }
    }
    Ok(())
}
