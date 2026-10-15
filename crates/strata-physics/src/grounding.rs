//! Grounding adapters over the collision-resolution world surface, without a renderer.
use ashlar::{ConvexSolid, GroundError, TerrainPatch};
use avian3d::prelude::Collider;
use bevy::math::{Quat, Vec3};
use strata_worldgen::WorldSource;

/// Sample a bounded one-metre, world-aligned patch matching LOD0 heightfields.
pub fn sample_patch(source: &dyn WorldSource, at: [f64; 2]) -> Result<TerrainPatch, GroundError> {
    let origin = [at[0].floor() - 40.0, at[1].floor() - 40.0];
    let mut heights = Vec::with_capacity(81 * 81);
    for z in 0..81 {
        for x in 0..81 {
            heights.push(f64::from(
                source
                    .sample(origin[0] + f64::from(x), origin[1] + f64::from(z))
                    .height,
            ));
        }
    }
    TerrainPatch::new(origin, 1.0, [81, 81], heights)
}

/// Convex compound made from the same corners that the render adapter draws.
/// A fitted site's prisms and the proxies a building's elements declare are
/// both `ConvexSolid`, so one building is one compound. Reject malformed
/// public data instead of substituting a bounding box.
pub fn collider(solids: &[ConvexSolid]) -> Result<Collider, String> {
    if solids.is_empty() {
        return Err("empty building collision".into());
    }
    let mut parts = Vec::with_capacity(solids.len());
    for solid in solids {
        let vertices = solid
            .vertices
            .iter()
            .map(|p| p.as_vec3())
            .collect::<Vec<_>>();
        if vertices.iter().any(|v| !v.is_finite()) {
            return Err("non-finite collision vertex".into());
        }
        let shape = Collider::convex_hull(vertices).ok_or("degenerate building collision")?;
        parts.push((Vec3::ZERO, Quat::IDENTITY, shape));
    }
    Ok(Collider::compound(parts))
}
