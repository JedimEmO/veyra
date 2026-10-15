# Modular geometry and material study

2026-09-13. First detailed facade bay and service entrance authored through the
Rust recipe API. This is an iteration baseline, with style decisions confined
to `crates/veyra-buildings/src/lib.rs` and the separate material library.

## Captures

- [Facade bay](facade.png): physical chamfers, recessed glazing, cast tie holes,
  layered frame/sill, vent slats, and separate material on exposed cut faces.
- [Entrance](entrance.png): recessed double panels, reveal linings, threshold,
  lintel, emissive strip, terminal and raised identifier.
- [Outpost](outpost.png): single-storey structure with a replacement-panel finish.
- [Office](office.png): three-storey structure using the same facade and entrance.
- [Clay wireframe](facade-wire.png): the facade loaded from exported RON, showing
  the actual chamfers and subtraction topology without textures.

The facade is 936 triangles; the entrance is 1,316. Both complete structures use
seven shared part definitions totalling 3,088 unique triangles. The outpost has
24 instances, the office 61. These are shared triangle counts, not total drawn
triangles; repeated parts still contribute geometry each time they are rendered.
Material/surface batches share mesh handles across instances.

## Reproduce and edit

```sh
just building-recipe-preview --scene facade --zoom 0.78 --yaw -0.35 --pitch 0.1 --screenshot docs/research/building-materials/facade.png
just building-recipe-preview --scene entrance --zoom 0.78 --yaw -0.3 --pitch 0.08 --screenshot docs/research/building-materials/entrance.png
just building-recipe-preview --scene outpost --zoom 0.95 --screenshot docs/research/building-materials/outpost.png
just building-recipe-preview --scene office --zoom 0.95 --screenshot docs/research/building-materials/office.png
just building-recipe-preview --scene facade --write-example /tmp/material-study-facade.ron
just building-recipe-preview --recipe /tmp/material-study-facade.ron --materials assets/buildings/recipe-study/materials.ron --clay --wireframe --zoom 0.78 --yaw -0.35 --pitch 0.1 --screenshot docs/research/building-materials/facade-wire.png
```

`P` toggles materials/clay, `F` wireframe. Drag left/right to orbit/pan, scroll to
zoom, Home to reset the chosen initial view. Material edits only require a
restart: edit `assets/buildings/recipe-study/materials.ron`. Slot bindings and
instance finish overrides live in the Rust recipe or its exported RON.

The owned tileable textures are generated in Rust by `textures.rs`, with no
external asset downloads. Color textures are sRGB; normal and packed ORM maps
are linear. Metre-based repeat sizes and UV offsets are material properties.
The replacement panel has a separate tint, repeat size and offset, sharing the
same mesh. Run `--write-study-assets` only when deliberately regenerating the
textures and material RON; it overwrites that study content.

## Verification and limits

Tests check chamfer bounds and closed surfaces, measured cut-face area and
material provenance, material validation/interchange, identical reused modules
across both layouts, finite normal-map tangents and tile boundary periodicity.
GPU captures check texture loading, shading and source-file preview. The full
repository check runs with the existing flora stack workaround:
`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=8388608 just --tempdir /tmp ci`.

The chamfer operation is explicit box geometry, not a general bevel modifier.
UVs are dominant-plane projections with seams, not a production atlas. Glass is
opaque reflective material; emission does not cast light. Wear is in the example
maps and finish overrides, not a layered decal system. Architectural composition,
transparent materials, general bevels and collision/LOD export remain separate
work. [Terrain integration](../building-grounding/README.md) now places the shared
Rust outpost in the actual world.
