# ADR 0003: Chunked quadtree heightfield with skirts, async meshing

Status: accepted, 2026-09-11.

## Context

The slice needs a large draw distance over a procedural world with varying biomes, at a stable frame rate, on a world that only exists as a sample function. Later needs: a heightfield collider, persisted terrain edits in some places, interiors and caves as separate cells, and the same region grid serving as the server's interest-management unit.

## Options

1. **Geometry clipmaps.** Best triangle efficiency and constant memory, but the mesh is camera-centred and continuous, which fits badly with per-region loading, per-region edits and a server that wants a region grid. Rejected for now.
2. **Voxel/marching cubes.** Gives caves and overhangs, costs an order of magnitude more memory and meshing time and makes biome-driven relief harder to author. Rejected: caves become cells (separate meshes attached to the heightfield) later, as Elder Scrolls does.
3. **Chunked quadtree heightfield (CDLOD-style selection, fixed grid per node, skirts).** Each node is a square region; the root covers the streamed radius; selection walks the tree splitting nodes whose distance to the observer is under `k * node_size`. Every node meshes into the same `N x N` vertex grid, so a far node is a coarse mesh and a near one is fine. Cracks between LODs are hidden with a skirt of vertices dropped below the surface at each edge. Nodes are entities, meshed asynchronously, and unloaded when deselected.

## Decision

Option 3. Concretely:

- `RegionCoord { x, z, lod }` with `size = 64 m * 2^lod`, grid `N = 65` vertices per side, so the finest node is one vertex per metre and lod 8 covers 16 km per node. Six to nine levels give a horizon of 30 km and beyond.
- Selection in `strata-stream` is a pure function of observer positions and a `StreamConfig` (split factor `k`, max lod, budget) and returns a wanted set; it has no Bevy render types and is tested headless.
- Meshing in `strata-terrain`: a task per node on `AsyncComputeTaskPool`, sampling `worldgen.sample_grid` for `(N+2)^2` points (one extra ring for normals), building positions, normals, a biome weight attribute and a material index attribute. Tasks are cancelled by dropping them when a node leaves the wanted set. A per-frame budget bounds how many results are inserted.
- Skirts, not stitching, in the slice. Stitching (index buffer variants per neighbour LOD) is a contained change inside the mesh builder if skirts ever show.
- Normals come from sampling, not from the mesh, so they are continuous across nodes and LODs.
- Height is sampled at the node's own grid, so a coarse node's height is not an average of the fine one. That produces small vertical popping on LOD change; morphing between LODs in the vertex shader (the CDLOD part) is a Phase 3 candidate if popping is visible in the slice.

## Consequences

- Region granularity is shared: the server can use the same coordinates for interest management and for persisting edits, and a heightfield collider maps onto a lod 0 node directly.
- Memory scales with the number of selected nodes, not with the world, and the number of selected nodes is roughly constant for a given `k` and view distance.
- The `worldgen` sample function is called on the compute pool from many threads at once; it must be `Send + Sync` and allocation-free per sample. This is a design constraint on `strata-worldgen` from the start.
