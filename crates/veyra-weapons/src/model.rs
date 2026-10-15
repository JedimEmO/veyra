//! What the game needs to know of a meshed weapon part beyond its triangles.
use ashlar::TriangleMesh;

/// How far back from a part's greatest Z a vertex still counts as its tip,
/// in metres. A flared or crowned muzzle has a ring of vertices at its end,
/// not one, and a millimetre keeps the ring and drops the taper behind it.
const TIP_DEPTH_M: f64 = 0.001;

/// The point a bolt leaves from when this part is the weapon's front: the
/// centroid of the vertices within a millimetre of the greatest Z of the
/// part's meshes, in the chassis's own frame (the muzzle is at +Z).
///
/// The game finds its flash and bolt start from this instead of a number
/// written beside each weapon, so a muzzle module that is longer moves the
/// flash with it. `None` for a part with no vertices.
#[must_use]
pub fn muzzle_tip<'a>(
    meshes: impl IntoIterator<Item = &'a TriangleMesh> + Clone,
) -> Option<[f32; 3]> {
    let front = meshes
        .clone()
        .into_iter()
        .flat_map(|mesh| mesh.positions.iter())
        .map(|p| p.z)
        .fold(f64::NEG_INFINITY, f64::max);
    if !front.is_finite() {
        return None;
    }
    let (mut sum, mut count) = ([0.0_f64; 3], 0.0_f64);
    for p in meshes
        .into_iter()
        .flat_map(|mesh| mesh.positions.iter())
        .filter(|p| p.z >= front - TIP_DEPTH_M)
    {
        sum = [sum[0] + p.x, sum[1] + p.y, sum[2] + p.z];
        count += 1.0;
    }
    // The narrowing is to the game's `f32` render space; metres on a weapon
    // lose nothing there.
    #[allow(clippy::cast_possible_truncation)]
    Some(sum.map(|s| (s / count) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Assembly, Finish, Slot, Weapon, build};
    use ashlar_manifold::{ManifoldMesher, mesh_building};
    use std::collections::BTreeMap;

    fn muzzle(module: Option<&str>) -> [f32; 3] {
        let assembly = Assembly {
            weapon: Weapon::PatrolCarbine,
            modules: module
                .map(|id| BTreeMap::from([(Slot::Muzzle, id.to_owned())]))
                .unwrap_or_default(),
            finish: Finish::Issued,
        };
        let building = build(&assembly).expect("the recipe");
        let meshed = mesh_building(&building, &ManifoldMesher::default()).expect("the mesh");
        let part = &building.instance("muzzle").expect("a muzzle instance").part;
        muzzle_tip(meshed.parts[part].iter().map(|element| &element.mesh)).expect("a tip")
    }

    #[test]
    fn the_carbines_muzzle_tips_are_where_the_barrel_ends() {
        let standard = muzzle(None);
        assert!(standard[0].abs() < 0.001, "x {standard:?}");
        assert!((standard[1] - 0.071).abs() < 0.001, "y {standard:?}");
        assert!((standard[2] - 0.49).abs() < 0.05, "z {standard:?}");
        let compensator = muzzle(Some("vigil_compensator"));
        assert!(compensator[2] > standard[2], "{compensator:?} {standard:?}");
    }
}
