# ADR 0005: Vegetation placed per cell below the render layer, grass through per-cell instance buffers, trees as shared LOD meshes

Status: accepted, 2026-09-12.

## Context

The world is a sample function and the terrain streams around every observer in 64 m regions (ADR 0003). Vegetation has to come from the same function, because a tree is a collider on the server and a mesh on the client and the two must agree without a word on the wire. It also has to be cheap in a way the terrain never had to be: hundreds of thousands of blades and thousands of trees around every player, animated, at the frame rate the slice already has. A discovery pass (`docs/research/foliage-moodboard.md`) collected the techniques and a disposable spike (`docs/research/foliage-poc.md`, branch `spike/foliage-poc`) measured the ones Bevy 0.19.1 makes easy.

## Options

**Where placement runs.** On the GPU at draw time from density textures (Horizon Zero Dawn), on the render side per cell from a hash (the spike), or below the render layer as a pure function of the world that both the server and the client call. The GPU route cannot give the server a tree. The spike's route works but leaves the server with nothing.

**How grass is delivered.** One entity per tuft with Bevy's automatic batching; one `Mesh` asset per cell; or a compact per-cell instance buffer read by a vertex shader through a shared blade mesh. The spike measured the first at 45 ms of CPU for 88 k entities and the second at about 5 ms per megabyte of upload, because `Mesh::write_packed_vertex_buffer_data` writes one attribute of one vertex at a time into mapped write-combined memory; a 13.6 MB cell stalls a frame by 65 ms, invisible at walking speed and 4 fps at 200 m/s. A raw storage buffer uploads at memcpy speed and needs no mesh per cell.

**How trees are delivered.** Shared procedural meshes with `VisibilityRange` LODs under Bevy's automatic batching (the spike: 2.5 k trees at six entities each for about 3 ms), or a custom instanced pipeline. The spike found the cost is not the trees but their shadows: three LODs casting into four cascades to 20 km cost 17 ms, the near LOD alone 2 ms, the far card LOD alone 11 ms.

**Wind.** Per-plant animation in the vertex shader from painted weights (Crysis, GPU Gems 3 ch. 16), driven by a global vector plus a scrolling gust field (Ghost of Tsushima), or skinned plants. The first two are what a vertex shader can do at this scale; the spike's version of them ran shadows and motion vectors from the same bend.

## Decision

1. **Placement is a pure function below the render layer.** A new Bevy-free crate `strata-flora` (above `strata-worldgen` and `strata-registry`, beside nothing) exposes `place_cell(coord, source, species, seed) -> CellFlora`: for one 64 m cell, the trees and bushes as `(species, position, yaw, scale, variant)` and the grass as a compact per-slot record, from a jittered grid per species thresholded by a per-slot hash against the species' density at that point (biome weights, slope, altitude, distance to the sea), big things first with exclusion radii, grass last. Randomness is `strata-core` hashing of seed, cell and slot, never a stream, so server and client agree bit for bit, and the crate keeps worldgen's rule of no platform transcendental per sample. Species are `.flora.ron` files in the registry, each naming the biomes it grows in with a weight, so a species (or a mod's) is added without editing a biome.

2. **Flora hangs off the region grid.** A region at lod 0 gets grass and near trees, at lod 1 and 2 trees only, at lod 3 and above nothing; a region's flora is its cells' `place_cell` results at the detail its lod allows. The client's `strata-foliage` (a render crate) and the server's `strata-physics` both answer `RegionReady` for the regions they care about, on the compute pool, without gating readiness, and drop the work on `RegionUnload`, exactly as the collider does today. The physics crate gives every tree a capsule collider on lod 0 regions; nothing about a tree is replicated.

3. **Grass is a per-cell instance buffer, never a `Mesh`.** One shared blade mesh per LOD (a fixed number of blades of unit geometry, uploaded once) and one material instance per cell whose storage buffer holds the cell's blades at 12 bytes each (root offset, height, width, phase, stiffness, tint index); the vertex shader pulls its blade by `vertex_index / vertices_per_blade`, places it on the ground with the terrain normal and tint at the root, and bends it. A cell of 24 k blades is a 290 KB `write_buffer`. Empty slots collapse to a point. Mid range is a second shared mesh with fewer blades and vertices; far range is the terrain, whose tint the material already blends toward. Bevy's material path (prepass, shadows, `VisibilityRange` crossfade, batching by material) stays in use; a compute pass that generates the buffer on the GPU is the step beyond this if the CPU build ever shows.

4. **Trees are shared procedural meshes with three LODs under `VisibilityRange`, and only the near LOD casts shadows.** The generator (Weber and Penn reduced to a species' parameters: levels, branch factor, taper, split angle, card size) produces a few variants per species at build time, each as a bark mesh and a masked leaf mesh at full, reduced and card detail; an octahedral impostor replaces the card LOD once the baker exists. Shadow cascades stop where trees are cards.

5. **Wind is one uniform from one resource.** `Wind { direction, strength, gust_scale }` lives in `strata-sim` so the simulation (and one day the server's weather) owns it; the foliage material's extension carries it with the time, the previous frame's time and the render origin wrapped to a period, so gusts are sampled in world coordinates and survive a rebase. Main pass and prepass evaluate the same bend, so shadows follow the wind and TAA gets true motion vectors. Weights ride in two vertex attributes: root and height fraction, phase and stiffness and flutter.

6. **Shading stays on `StandardMaterial`.** Diffuse transmission for blades and leaves, mask alpha (alpha-to-coverage under MSAA), the terrain's normal and tint at a blade's root, canopy-sphere normals baked into leaf cards, a per-instance tint from the hash. Alien species differ in the generator's parameters, the palette and an emissive term, not in the renderer.

## Consequences

### Amendment: persistent client cells, 2026-09-12

Decision 2's client ownership is superseded: `strata-foliage` owns persistent
64 m cells around the primary observer, independently of terrain quadtree
entities. Terrain readiness permits initial dispatch, but a terrain split or
merge never removes foliage. Physics continues to own its colliders by region.
Cells retain their placement, growth timestamp and installed LODs while finer
detail is added. A 128 m preparation band and another 128 m retention band
avoid boundary churn. Evicted CPU placements are cached within both a 32 MiB
and a 256-cell limit, scoped to the current world and frozen species table.

Placement jobs are one cell each, with eight outstanding and four dispatched
per frame by default. Collection resumes per tree LOD, with 600 entity spawns,
600 removals, a 1.5 ms command-generation budget and 1 MiB of grass uploads per
frame. One oversized cell upload is allowed alone to guarantee progress.
Deferred entity application is timed separately; the time budget is soft at
an atomic tree or cell upload boundary. Retirement hides a cell immediately
and removes its entities incrementally. Grass records remain 16 bytes.

Decision 4 now uses a bounded CPU bake of each actual near variant into eight
azimuth views, with masked color and normal atlases and coverage-preserving
mips. This replaces the generic crossed far cards. It is an eight-view
impostor, not the deferred octahedral/elevation-blended baker. Reduced meshes
retain the near variant's skeleton, and pines have a leader and tapered whorls.
Only the near tree LOD casts shadows.

Grass adds continuous 24 m and 7 m patches and 2 m tuft variation below the
render layer. Species data controls width, curvature, droop and dead blades.
Quantized roots sample the same triangle diagonal as the finest terrain and
physics grid. Both CPU and GPU LOD evaluation use a cell origin at ground
height. Wind uploads after origin sync, with current and previous origin
offsets and a genuinely periodic 4096 m gust field for consistent motion
vectors through rebases.

### Amendment: review hardening, 2026-09-12

Species randomness is keyed by a stable hash of the namespaced asset ID,
never the dense registry index or species enumeration order. Every plant
carries a `PlantId` comprising that species key, its cell and its candidate
slot. Physics and rendering preserve that identity for later interactions.
It is scoped to a world; saved edits will also need the world/generation
version. Adding unrelated content cannot change existing species shapes.

Only `FloraKind::Tree` produces a trunk collider. Decorative bushes, flowers,
ferns and logs do not inherit cylinders from the geometry generator.
Physics installs at most 256 trunks per frame, retaining unfinished results
by region and dropping them when the region unloads.

Production placement includes neighboring candidates. Local priority
thinning makes each exclusion decision depend on a finite radius, rather
than a recursive greedy chain across cells. Trees outrank bushes; other
priorities derive from stable candidate identities. Neighbor edge samples
share the same terrain triangles as the center grid. Physics reuses its
center samples and calls the same source-aware placement function as the
client. The grid-only placement helpers are for isolated fixtures and do
not provide cross-cell exclusion.

Registry freeze validates an aggregate maximum of 65,536 grass candidate
slots and 4,096 plant candidate slots per cell, 32 grass palette entries,
and a maximum exclusion radius of 16 m. Placement also bounds manually
constructed tables. A deterministic, evenly distributed selection caps
accepted grass at 32,768 records before either LOD consumes it, so a cell's
buffer is at most 512 KiB. Grass palette indices count only grass species;
unrelated global registry IDs consume no shader slots. Oversized mesh
requests and invalid palette references are no longer silently aliased.

Client cell roots and far trees remain persistent, but grass and finer tree
LODs retire independently after leaving their own preparation plus retention
bands. Entity ownership is tracked per LOD; all removals share the frame
budget, including a cell eviction while its detail is already retiring.
Resident CPU records, resident grass buffers and grass buffers awaiting
retirement have separate byte counters. These counters exclude shared
meshes, textures, material metadata, and placement tasks.

Tree mesh conversion and impostor rasterization run on the compute pool.
The main world installs at most one prepared mesh or image per frame and
publishes the shared tree table only after installation completes. One
bounded asset remains the atomic upload unit. The grass grow-in timestamp
is a full elapsed time in the cell material instead of a repeating 16-bit
mesh tag, so a long-lived cell never grows in again after 109 minutes.

This amendment intentionally changes procedural placement and shape output;
the seed-42 placement snapshot is updated. Existing authored species files
remain valid, but content exceeding the explicit limits now fails at load.

See `docs/research/foliage-improvements.md` for validation and limitations.

- Two new crates: `strata-flora` (no Bevy) and `strata-foliage` (render). `just server-check`'s allowlist gains nothing; the flora crate is plain Rust and the physics crate's tree colliders are `parry` shapes it already has.
- `strata-worldgen::Sample` grows what placement needs and the terrain does not: a moisture or fertility scalar is the first candidate. Anything placement reads must be as deterministic as height.
- The anti-aliasing choice stays open and foliage does not force it: MSAA 4x (Bevy's default, what the client runs) and TAA measured the same in the spike, and the prepass shader produces the motion vectors TAA needs. Everything under this ADR has to keep working under both until a camera and post ADR picks one.
- Grass exists only on the client; a server never builds it. Trees exist on both, and a tree the client draws is a tree the server collides with.
- Bevy's large-mesh upload cost is a constraint on every future mesh, not only grass: keep per-frame mesh uploads under a megabyte or write buffers directly.
- Placement cost per cell is bounded by the species list and the grid; the spike's 65 x 65 height sample per cell (the same grid the collider samples) is the budget to keep.

### Amendment: the world is the seed and a baked tree library, 2026-09-26

Decision 4 is superseded for trees. Trees are no longer grown at runtime
from a species' parameters. The content step (`just content`) grows every
variant of every tree species with `veyra-flora`: space colonisation for
broadleaves and shrubs, whorls for conifers, pipe-model radii, per-vertex
wind and shade data and a per-variant pivot table. It bakes each variant to
an ashlar prop (`generated/flora/<species>-<variant>.ashprop`) with three
mesh levels (60, 150 and 350 m for trees) and a hemi-octahedral impostor
last level. It then writes `generated/flora/library.ron`: per species id,
the variants in order with their prop, height and trunk.

The world is therefore the seed **and** the library. Placement still picks
a species and a variant from the seed (Decision 1), but a baked species
takes its variant count and its trunks from the library, so the server's
collider is the trunk the client's mesh was grown with. The library's
`version` is an FNV-1a hash of its contents with the version zeroed. It is
checked when the file is read, and it joins the replicon protocol hash
(`ReplicationPlugin::with_tree_library`), so a client with another library
is refused at the handshake. The server and a connecting client read the
library before they run, and exit with the message to run `just content`
when it is missing; the world loader reads it again for placement, with
`std::fs` and no asset server, so the server needs none.

On the client, near trees are entities per mesh level with a
`TreeMaterial`: the ashlar material its piece names, extended with the
variant's pivot table. The vertex shader walks from a vertex's pivot out to
the trunk, at most four pivots, turning each branch about its attachment
by an angle weighted by where the point it carries sits along it, so
junctions stay closed. The same bend runs in the prepass and shadow passes.
Only the nearest level casts shadows.

Far trees are not entities. The Phase 8 capacity test drew an impostor per
tree and was CPU-bound at 700 000 entities at forest range. Instead each
64 m cell draws one batch per species: a shared quad mesh whose slots read
the cell's instance buffer, as grass does, with a material over the
species' atlas. Each tree fades in on its own root distance with the
dither pattern Bevy fades its last mesh level out with, so the handover
is exact per tree. The atlas is baked from a species' first variant and
scaled to each variant's height, because an atlas per variant costs six
times the texture memory until atlases are block compressed.

The understory (flowers, ferns, bushes, logs) is still grown at runtime
until Phase 10 bakes it. It is converted into the same vertex contract
with a one-pivot table and wears the baked bark and leaf materials, so
there is one tree material and one shader. The photographed bark sets
under `assets/textures/` are deleted, along with the runtime tree path of
`build_tree`, the procedural leaf texture and the startup impostor baker.

### Amendment: rocks, groves and children, 2026-09-27

Placement grows three rules, all data in a species' `.flora.ron` and all
pure functions of the seed, so the server's colliders follow them unchanged.

- **Rocks are a kind.** A `rock` species (boulders, stones, cliff pieces baked
  by `veyra-rocks`) is placed before trees and wins every exclusion against
  them. It reads the ground's shape through `ground`: a slope floor, a
  preference for hollows or knolls and for erosion ridges or drainage lines
  (the landform of ADR 0006), and `face_downhill`, which turns a cliff
  piece's face (`+z` in its frame) down the slope.
- **Groves and clearings.** `cluster` keeps every candidate's grid slot and
  only changes which survive: clump centres hashed on a 32 m world lattice
  each lend a smooth bump, normalised so the species keeps its density on
  average, and a patch field clears what falls under its cover. Adding a
  species' clumps moves no plant that stays. A test measures the pair
  correlation of forest trees within 20 m: 0.94 on the old grid, 1.35 with
  clumps. Size follows fit for what grows, so a grove's edge holds smaller
  trees.
- **Children.** `children` names species a placed plant brings with it:
  ferns under an oak, stones round a boulder or below a cliff. A child is
  hashed from its parent's identity and placed by the cell it lands in, from
  the parents that cell's halo already holds, so it is where it is whatever
  streamed first. It keeps clear of every plant but its parent.

The baked library's trunk became a general collider: an upright cylinder
(a trunk), a capsule (a fallen log), convex hulls (a rock, or a cliff's
pieces) or none (a flower). The physics builds it at the plant's root,
turned by the plant's yaw. The understory moved into `veyra-flora` and the
library, so flowers, ferns, bushes and logs are baked like trees and drawn by
the same material; `build_understory` is only the fallback for a world with
no content step.

### Amendment: shared buffers and one level a plant, 2026-10-04

Lighting plan phase 6 (`docs/perf/profile-2026-10-03.md`) found the render
thread, not the GPU, bound the frame, and most of its cost was per entity,
per batch set and per new `Mesh` asset. Three parts of this ADR changed to
pay for it.

- **Grass: one material over shared buffers.** Decision 3's "one material
  instance per cell whose storage buffer holds the cell's blades" is
  superseded. Every grass cell now draws with the one `GrassMaterial`, over
  two storage buffers shared by every resident cell: the blade records (16
  bytes each, as before) and one 16-byte record per cell saying where its
  blades start and how many there are (`grass::GrassStore`). A cell's
  entities carry its slot in the cell buffer as their `MeshTag`, and the
  shared blade meshes come in size buckets (2 048 to 32 768 slots) so a
  cell takes the smallest that holds it. A material per cell was a batch
  set per visible cell in every pass, each a bind group and a bin
  unpacking dispatch a frame, and a material re-prepared for every cell
  that streamed in; one material is one set a pass. Freed ranges wait a
  few frames before reuse, for the frame the render world is still
  drawing. Installing a cell is still two `write_buffer`s and no asset.
- **Far trees: one buffer for the whole ring.** The 2026-09-26 amendment's
  per-cell instance buffer is superseded. All far trees live in one
  growing storage buffer (`impostor::FarTreeStore`, 32 bytes a tree) that
  each species' one `FarTreeMaterial` binds. A batch (one species in one
  cell, as before) owns a range of it whose start rides in its `MeshTag`,
  and its mesh is one of thirteen shared quad meshes sized in powers of two
  (1 to 4 096 slots), the slots past its trees reading empty records that
  collapse. A mesh per batch was 150 to 240 new `Mesh` assets a second
  while the far ring filled, the render thread's largest system on any
  moving pose; installing a batch now creates no asset. Each tree still
  fades in on its own distance with Bevy's dither, so the handover from
  the last mesh level is exact per tree.
- **Near trees: a plant holds the level it draws.** Decision 4's three
  LODs under `VisibilityRange` remain how a level fades, but a plant no
  longer holds all of them at once. The streamer
  (`streaming::plant_levels`) makes a level when the camera comes within
  its band widened by a lead (8 m plus a quarter second of the camera's
  speed) and drops it once the camera is a margin (8 m) past the band and
  the level has lived its least dwell (1 s). So a plant holds one level,
  two through a crossfade, instead of three with `VisibilityRange`
  choosing: Bevy pays for every entity, visible or not, in visibility,
  range checks, change scans and extraction. Only the near level casts
  shadows, as before.

The world and the server are unchanged: placement, the library and the
colliders are what they were, and nothing here is replicated.
