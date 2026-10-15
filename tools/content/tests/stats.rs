//! What each shipped building costs, snapshotted, and held to the budget.
//!
//! A change to a kit that moves these numbers shows up as a snapshot diff in
//! review; one that breaks the budget fails here, before it reaches a frame.
use ashlar_manifold::ManifoldMesher;
use content::{BuildingStats, budget, building_stats, buildings};

fn stats() -> Vec<BuildingStats> {
    buildings()
        .expect("the recipes validate")
        .iter()
        .map(|(path, building)| {
            let baked =
                ashlar_manifold::bake(building, &content::ladder(), &ManifoldMesher::default())
                    .expect("the building meshes");
            building_stats(path, &baked)
        })
        .collect()
}

#[test]
fn building_stats_are_reviewed() {
    insta::assert_ron_snapshot!(stats());
}

#[test]
fn every_building_is_within_budget() {
    for building in stats() {
        let near = &building.levels[0];
        let far = building.levels.last().expect("at least one level");
        assert!(
            near.drawn_triangles <= budget::NEAR_DRAWN_TRIANGLES,
            "{}: {} triangles drawn near",
            building.path,
            near.drawn_triangles
        );
        for (index, level) in building.levels.iter().enumerate() {
            assert!(
                level.pieces <= budget::PIECES_PER_LEVEL,
                "{} level {index}: {} pieces",
                building.path,
                level.pieces
            );
        }
        // Counts of triangles, far below 2^52; the casts are exact.
        #[allow(clippy::cast_precision_loss)]
        let share = far.drawn_triangles as f64 / near.drawn_triangles as f64;
        assert!(
            share <= budget::FAR_SHARE,
            "{}: the far level keeps {:.0}% of the near level",
            building.path,
            share * 100.0
        );
        assert!(
            building.colliders > 0,
            "{}: nothing to stand on",
            building.path
        );
    }
}
