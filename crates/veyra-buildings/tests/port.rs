//! The street fittings have a furniture budget and must survive the same
//! distance preparation and coplanar-face audit as the buildings they dress.
use ashlar::{Building, Instance, LodPolicy};
use ashlar_manifold::{ManifoldMesher, bake, mesh_building};
use veyra_buildings::port::Fitting;

#[test]
fn port_fittings_are_small_shared_parts_without_fighting_faces() {
    for fitting in Fitting::ALL {
        let part = fitting.part().expect("a valid fitting");
        let mut builder = Building::builder("test:port");
        for slot in part
            .elements
            .iter()
            .map(|e| &e.material_slot)
            .collect::<std::collections::BTreeSet<_>>()
        {
            builder = builder.material(slot, format!("test:{slot}"));
        }
        let building = builder
            .part(part)
            .instance(Instance::new("one", fitting.key()))
            .build()
            .expect("bindings");
        let mesher = ManifoldMesher::default();
        let baked =
            bake(&building, &LodPolicy::ladder(), &mesher).expect("all distance levels mesh");
        assert!(
            baked.triangles()[0] < 1500,
            "{fitting:?}: {:?}",
            baked.triangles()
        );
        let near = mesh_building(&building, &mesher).expect("near level");
        let fighting: Vec<_> = near
            .coplanar_overlaps()
            .into_iter()
            .filter(|overlap| !overlap.covered && !overlap.same_binding())
            .collect();
        assert!(fighting.is_empty(), "{fitting:?}: {fighting:?}");
    }
}
