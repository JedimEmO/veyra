# Foliage spike: results

Status: done, 2026-09-12, on branch `spike/foliage-poc`. Disposable: the
branch is mined for the plan and not merged. Companion to the mood board in
`foliage-moodboard.md`.

## What was built

A `strata-foliage` crate on the render side: 64 m cells around the primary
observer, a compute-pool task per cell that samples a 65 x 65 height grid
and places trees (jittered grid, hash-thresholded, slope and sea-level
rejected) and grass (jittered grid at a density, thresholded by a per-slot
hash, kept clear of trunks). Grass is one mesh per cell (`patch`) or one
entity per shared tuft mesh (`tufts`). Trees are four procedural variants
(recursive tubes plus leaf cards) at three LODs under `VisibilityRange`,
each LOD a bark entity and a masked leaf entity. One material extension
over `StandardMaterial` does the wind in its main-pass and prepass vertex
shaders from two extra vertex attributes (root, height fraction, plant
height; phase, stiffness, flutter) and a wind uniform whose gust noise is
sampled in world coordinates through a wrapped origin offset. Cells carry a
`WorldPosition` and everything under them is cell-local, so the origin sync
rebases them like terrain.

`game` gained flags for the variants: `--aa msaa4|off|taa`,
`--grass-mode patch|tufts`, `--leaf-alpha mask|a2c`, `--grass-density`,
`--tree-spacing`, `--grass-radius`, `--tree-radius`, `--shadow-lods`,
`--transmission`, `--wind`, `--grass-shadows`, `--foliage-budget`,
`--large-mesh-mib`, `--rebase-threshold`, `--no-foliage`.

## Numbers

All at 1920 x 1080, release, the default spawn with the orbit camera,
settled after streaming; 214 cells, 530 k blades, 2 479 trees, 17.6 k
foliage entities unless stated. Frame time is the client's own two second
average.

| Run | avg ms | fps |
|---|---|---|
| no foliage | 5.6 | 180 |
| patch grass, trees, all LODs cast shadows | 23.6 | 42 |
| same, shadows off | 7.0 | 144 |
| same, only the near tree LOD casts | 8.9 | 113 |
| same, near and mid LODs cast | 12.5 | 80 |
| trees only (no grass), all LODs cast | 23.6 | 42 |
| tree spacing 24 m (392 trees), all LODs cast | 6.7 | 150 |
| grass density 16 (1.41 M blades), all LODs cast | 25.8 | 39 |
| tufts (88 k grass entities, 105 k total) | 63 to 67 | 15 |
| alpha to coverage instead of mask | 26.4 | 38 |
| TAA instead of 4x MSAA | 26.8 | 37 |
| no anti-aliasing | 23.6 | 42 |
| transmission 0 instead of 0.4 | 24.0 | 42 |

Flight at 200 m/s, 120 m up, near LOD shadows only:

| Run | avg ms | fps |
|---|---|---|
| no foliage | 6 to 8 | 130 to 165 |
| trees only | 15 to 18 | 55 to 65 |
| grass density 1 | 17 | 60 |
| grass density 2 | 39 | 26 |
| grass density 3 | 48 | 21 |
| grass density 6 (default), 4 cells per frame | 250 (capped) | 4 |
| grass density 6, 1 cell per frame | 80 | 13 |
| grass density 6, patches in their own GPU buffers | 250 (capped) | 4 |
| static flycam, same spot, density 6 | 11 | 90 |

Walking with the rebase threshold at 15 m (a rebase every few seconds):
9 to 11 ms, no hitch above 16 ms, grass and trees stay put.

## Answers

1. **Instancing path.** Per-entity grass is out: 88 k tuft entities cost
   45 ms of CPU on top of everything, and that is Bevy's automatic batching
   doing what it can. Trees as shared meshes are fine: 2.5 k trees at 6
   entities each add about 3 ms once shadows are sane. Per-cell patch
   meshes draw for nearly nothing (880 k more blades cost 2 ms) but cost
   about 5 ms per megabyte to *upload*: `Mesh::write_packed_vertex_buffer_data`
   in bevy_mesh 0.19.1 writes one attribute of one vertex at a time
   straight into the mapped staging buffer, which is write-combined
   memory. A default patch is 170 k vertices at 80 bytes, 13.6 MB, so a
   cell costs about 65 ms of frame time when it appears. At walking speed
   that is invisible; at vehicle speed it is a stutter per cell; at 200 m/s
   it is 4 fps. It does not depend on the mesh allocator (own buffers made
   no difference) and terrain never hit it because its meshes are small.
   Conclusion: grass must not go through `Mesh` assets at all. It wants a
   custom instanced pipeline whose per-blade data is generated on the GPU
   from a per-cell height texture (the Tsushima shape), or at the least a
   packed 16 to 24 byte vertex written by us into a buffer we own. Trees
   go through ordinary meshes and automatic batching.
2. **Anti-aliasing.** TAA and MSAA cost the same here and both look fine
   on blades; the masked leaves under MSAA with alpha-to-coverage look the
   same as with the mask at this scale. Bevy's TAA needs the depth and
   motion vector prepasses, and the foliage prepass shader already
   evaluates the previous frame's wind for motion vectors, so nothing in
   the foliage design forces the choice. It stays a camera and post
   decision; the plan can record MSAA 4x as the current default and TAA as
   compatible.
3. **Rebasing.** Cells under a `WorldPosition` with cell-local children
   survive rebases with no visible motion and no hitch. The one thing to
   carry is the wind field's origin offset uniform, so gusts do not jump.
4. **Material.** `StandardMaterial` with `diffuse_transmission` 0.4 reads as
   back-lit grass and leaves; at 0 the shaded side goes black. Mask and
   alpha-to-coverage both work through the terrain's depth prepass and
   the shadow pass with a custom prepass vertex shader. A material
   extension is enough; no material from scratch.

## Findings that were not questions

- **Shadow casters are the budget.** All three tree LODs casting into four
  cascades out to 20 km cost 17 ms; only the near LOD casting costs 2 ms.
  The far LOD (two masked cards per tree) alone is 11 ms. Shadow views do
  cull by `VisibilityRange` (Bevy picks the camera as the LOD origin by
  itself), so the cost is the sheer count of masked casters, not
  duplicated LODs. Rule for the plan: shadows from the near LOD only, and
  cascades that stop where trees are cards.
- Importing `visibility_range_dither` by name into a shader that also
  declares a struct field of that name breaks the field under naga_oil's
  renaming; import the module and call it qualified. The terrain shader
  has the same latent bug and never compiled that variant.
- A `[f32; 2]` in a `ShaderType` uniform panics in encase at runtime;
  use `Vec2`.
- `bevy/trace_chrome` does not build against this lockfile
  (profiling-procmacros 1.0.18 is not on the index), so the profiler is
  unavailable; the upload cost above was bisected by scaling instead.

## What the plan should take

- Placement per 64 m cell from a deterministic per-cell hash, big things
  first with exclusion, grass last; below the render layer so the server
  can place tree colliders from the same function.
- Trees as shared procedural meshes with three LODs under
  `VisibilityRange`, near LOD shadows only, a card LOD until impostors.
- Grass as a GPU-generated instanced pipeline per cell, never a `Mesh`.
- One wind uniform with time, previous time and a wrapped origin offset,
  applied by main-pass and prepass vertex shaders.
- `StandardMaterial` extension with diffuse transmission for leaves and
  blades; mask alpha; alpha-to-coverage when MSAA is on.
