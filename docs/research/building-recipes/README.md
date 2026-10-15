# Rust recipe mesh preview

Captured 2026-09-13 from `building-preview`, using the Rust example at
`crates/veyra-buildings/src/example.rs` and the Manifold adapter. This is a
geometry verification fixture, not an approved architectural style or asset kit.

- [Shaded preview](shaded.png): open door/window cuts, smooth cylinders, shared
  facade modules and a concave extruded canopy.
- [Wireframe preview](wireframe.png): the same assembly loaded from exported RON.

The fixture has six used part definitions, 29 instances and 692 unique triangles.
Each element has one shared GPU mesh handle across its instances. The alternate
preview palette and arbitrary material-key colors are diagnostic materials.

Reproduce from the repository root:

```sh
just building-recipe-preview --scene basic --screenshot docs/research/building-recipes/shaded.png
just building-recipe-preview --scene basic --write-example /tmp/building-preview-example.ron
just building-recipe-preview --recipe /tmp/building-preview-example.ron --wireframe --screenshot docs/research/building-recipes/wireframe.png
```

Verification covers signed volumes, exact closed surface edges and consistent
winding, UV/normal validity, cylinder smoothing and cap creases, concave profiles
in both windings, doorway cuts, nested rotated differences, empty/disjoint cuts,
resource limits and shared part/material semantics. A headless preview test
checks camera zoom/reset and palette/wireframe toggles. Full repository CI uses
`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=8388608 just --tempdir /tmp ci`; the increased
stack is the existing workaround for the unrelated flora test instability.

The screenshots verify generation and preview, not runtime world integration,
production materials, collision meshes or LOD export. See
[the authoring workflow](../../BUILDING_RECIPES.md) for those boundaries and the
native build prerequisites.
