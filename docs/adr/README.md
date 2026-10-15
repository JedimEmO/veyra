# Architecture decision records

One file per decision, numbered, never edited after acceptance except to mark them superseded.

| ADR | Decision |
|---|---|
| [0001](0001-crate-layering-and-authority.md) | Layered crates, one simulation, server and client as plugin groups |
| [0002](0002-world-coordinates.md) | `f64` world positions with a movable render origin |
| [0003](0003-terrain-lod.md) | Chunked quadtree heightfield with skirts, async meshing |
| [0004](0004-networking-and-prediction.md) | Replication through an adapter, interest on the region grid, snap-based prediction, a world that does not pause |
| [0005](0005-foliage.md) | Vegetation placed per cell below the render layer, grass through per-cell instance buffers, trees as shared LOD meshes, wind as one uniform |
| [0006](0006-rainfall-hydration-cover.md) | Rainfall marched upwind over relief per sample; hydration and cover as sample fields |
| [0007](0007-render-look-studies.md) | Exported render looks, camera effect ownership and fixed local atmosphere studies |
| [0008](0008-building-recipe-domain.md) | Rust building recipes independent of geometry backends and visual style |
| [0009](0009-settlement-sites.md) | Settlements are placed and shaped by the world function |
| [0010](0010-river-network.md) | The river network: one bake per land mass below the render layer, read by the sampler |
| [0011](0011-geological-provinces.md) | Geological provinces: a registry and a field of their own beside the biomes (proposed) |
| [0012](0012-procedural-sites-and-roads.md) | Procedural sites placed by the bake and the roads between them; authored settlements stay fixed (amends 0009) |
