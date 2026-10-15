# Asset refactor and first world slice: implementation plan

Status: Phases 6 to 12 done 2026-09-27. The refactor is finished; the slice is passable except the frame-time gates of the two densest forest fixtures and the flight (section 1, Phase 12 record).
It follows the [asset pipeline discovery](research/asset-pipeline-discovery.md)
and its decisions, and continues [PLAN.md](PLAN.md) from Phase 6.
It replaces PLAN.md's note that rocks, props and the first interior cell "go to Phase 6".

## 1. Where this plan ends

Two outcomes, each with a definition we can check.

**The refactor is finished** when every asset the game draws comes from `just content`,
except grass, which stays runtime-procedural by design:

- No photo textures remain in `assets/`; terrain layers, bark, leaves and rock surfaces are graph bakes.
- `build_tree`, the startup leaf images and the startup impostor baker are gone from the game.
  Trees, understory and rocks load from a baked library.
- Neither `game` nor `server` links `ashlar-manifold` or `ashlar-material`,
  and `just server-check` asserts both, as well as `veyra-flora` and `veyra-rocks` for the server.
- The server derives every tree, rock and building collider from baked data it reads with plain `ashlar`.
- `just content` is deterministic: two cold runs write identical bytes,
  and a warm run with no changes rewrites nothing.

**The first slice is passable** when a player can walk from wilderness into a small metropolis and back,
and the whole route holds up in screenshots and in the frame-time log:

- A seeded settlement site near the spawn, on terrain shaped to receive it,
  with a dense core of towers from ashlar's city kit and a Veyra ring of outposts and habitats around it.
- One road from the wilderness into the city, with vegetation cleared along it.
- Forests, understory, rocks and cliffs on the way that read as organic and stylised,
  with no visible grid, no uniform spacing and no visible LOD pops in a still.
- Two clients connected to a server see the same city and collide with the same buildings, trees and rocks.
- Every performance gate in section 3 holds at every fixture, including the new city fixtures.

Out of scope: interiors you can enter beyond a lobby, residents, missions, roads between several settlements,
building damage, weather, the painterly shaders themselves, and the alien biome.
The plan leaves each of these a place to plug in and builds none of them.

## 2. Shape of the work

Seven phases.
Each one ends in a runnable game that is at least as fast as the one before it,
and each one has its own exit gate.
We do not start a phase until the previous gate is green.

| Phase | Delivers | ashlar work | Changes every seed |
| --- | --- | --- | --- |
| 6 | Green build, content step, buildings baked, bench and review harness | Cache in `ashlar-content`, maps in named sources | No |
| 7 | Terrain: erosion, attributes, stylised procedural layers, macro colour | None | Yes |
| 8 | ashlar organic foundations: alpha, atlases, vertex data, mesh-set asset kind, impostors | Most of it | No |
| 9 | Trees from `veyra-flora` through the content step | Small fixes | Yes |
| 10 | Scatter, understory, rocks and cliffs | Organic shapes in `Geometry` | Yes |
| 11 | The settlement: site, road, city, streaming, server colliders | Collider access without Bevy | Yes, near the site |
| 12 | Slice polish, acceptance and the documents | None expected | No |

This may look backwards: why terrain before the ashlar foundations the trees need?
Terrain shapes everything placed on it, so every later phase would otherwise tune against terrain that is about to change.
It also exercises the content step on the simplest asset kind, textures, before we bet the trees on it.

Phases 7 and 8 are independent and can overlap if we want two lines of work.
Phases 9 and 10 both need 8.

## 3. How every phase is verified

### Correctness

Every phase runs `just ci` and ashlar's own `just ci` when ashlar changed.
Beyond that, each phase lists its own tests; these kinds recur:

- **Determinism tests.** Placement, sites and generators give identical output for a seed.
  Where the world changes, the insta snapshots are reviewed and re-accepted in the same commit, never loosened.
- **Server agreement.** A headless test places the same cells as the client path and compares colliders by value.
  From Phase 9 on, the end-to-end check runs a `server` and two `game --connect --auto-walk` clients.
- **Asset stats.** The content step writes a stats file per asset: triangles per level, bounds,
  texel density, alpha coverage for cards and impostors, and collider count.
  insta snapshots it, and a test asserts each family's budget.
- **Review sheets.** A `just review` renders contact sheets, black silhouette sheets at 64 px and turntables
  for each asset family into `scratch/review/`.
  They are for looking at, not for pixel comparison; phase exits cite them.

### Performance

The dev machine's RTX 4090 at 2560x1440, release build, vsync off, MSAA 4x, is the reference.
We have not picked a minimum GPU, so until we do, the 4090 has to run everything at 120 fps.
We are betting that this leaves a mid-range card near 60 fps at 1080p; it is an assumption, not a measurement.

Phase 6 builds `just bench`, which runs each fixture for a fixed number of frames after warm-up
and writes p50, p95, p99, worst frame, GPU pass times, entity counts and upload bytes per frame to `scratch/bench/`.
It compares them to `docs/perf/ledger.md`, the committed record of every phase's exit numbers,
and fails when a gate is broken.

| Gate | Threshold | Why |
| --- | --- | --- |
| Frame time at every fixture | p95 at most 8.3 ms, p99 at most 12 ms | 120 fps stand-in for the undecided minimum GPU |
| Regression per phase | A phase may spend only its own row of the budget table | Stops one system eating the others' headroom |
| Streaming hitches | No frame over 33 ms in the flight and walk runs after the first 5 s | Streaming must never stall a frame |
| Upload per frame | At most 1 MiB, except the documented single large grass cell | Bevy 0.19 uploads meshes at about 5 ms per MB |
| Server tick | p99 at most 10 ms of the 50 ms tick, two players walking | Leaves room for gameplay on the 20 Hz sim |
| Placement per cell | At most 3 ms on one thread, as today | Cells must keep up with a sprinting player |
| Cold content bake | Under 10 minutes; a warm bake with no changes under 30 s | Iteration speed is the pipeline's reason to exist |

The frame budget, split by system at the busiest fixture.
Phase 6 fills in the measured baseline, and later phases must stay inside their row:

| System | Budget at the busiest fixture |
| --- | --- |
| Terrain (all passes) | 1.2 ms |
| Trees, near shadows included | 2.5 ms |
| Grass and understory | 1.2 ms |
| Rocks and cliffs | 0.6 ms |
| City and buildings | 2.0 ms |
| Atmosphere, water, post, UI | 0.8 ms |

A gate that the Phase 6 baseline already misses is carried, not waived:
the ledger names the phase that owns its cause, every phase exit before that
one must leave the number no worse, and the owning phase must bring it inside
the gate. Every other gate must hold at every exit.

These sum to 8.3 ms.
Not every system peaks at the same fixture: the city fixtures carry almost no grass,
and the woodland fixture carries no city.

The fixtures are the four look-study scenes (woodland, grove, valley, coast), the 12 km flight,
an auto-walk, and from Phase 11 on three city fixtures:
street level, a rooftop view across the city, and the approach road at the forest edge.

## 4. Phases

### Phase 6: green build, content step, harness

The refactor needs a base that builds, a content step to put assets through, and a way to measure.
Nothing visible changes in this phase, on purpose, because it is the baseline every later phase is compared against.

Work:

1. **Fix the build against ashlar HEAD.** `veyra-buildings/src/textures.rs:128-130`
   sets `MaterialDefinition` fields that no longer exist.
   Then fix the calls the build reaches next:
   `ashlar_bevy::read` and `create` are `read_library` and `create_material`,
   and `Catalog::asset_writer` is gone.
2. **`tools/content`**, a binary over `ashlar-content`, and `just content`.
   Generated files go to their own asset source, `generated://`, rooted at a gitignored `generated/`.
   This answers the discovery's open question: committed data and generated output can never mix,
   and deleting `generated/` is always safe.
   ashlar needs one change for it: `ashlar-bevy` loads and preflights material maps by bare key,
   which Bevy resolves in the default source, so a library under `generated://` would look for its maps in `assets/`.
   Keys should resolve in the library's own asset source.
3. **Buildings through the content step.** The grounding fixture loads `.ashlar` files through `AshlarPlugin`
   instead of meshing at startup, and `ashlar-manifold` leaves `apps/game`.
   Update CLAUDE.md, which still names the game's grounding fixture as a place the kernel may be linked.
   `veyra-buildings`' noise textures become material graphs baked to KTX2.
4. **A content cache in `ashlar-content`.** Each output is keyed by `blake3` of processor, version constant, parameters and seed.
   The cache lives under `target/content-cache/`, and a warm run skips unchanged outputs.
   A cache in ashlar is library work: any game with a content step needs it.
5. **`just bench`** and the perf ledger (section 3), built on the frame statistics the game already logs,
   with the look-study fixture positions from `tools/render-study.py`.
6. **`just review`** and the stats files (section 3), starting with the buildings.

Verify:

- `just ci`, ashlar's `just ci` and `just server-check` pass, with the new assertion that `game` links no kernel.
- The grounding study's six captures match `docs/research/building-grounding/` by eye.
- `just content` twice from cold writes identical bytes; a third, warm run writes nothing.

Exit gate: the ledger holds the baseline for every fixture and every gate in section 3 has a measured value.
Where today's numbers already miss a gate, the ledger records it and names the phase that fixes it.

**Done 2026-09-26.** What landed:

- The build is green against ashlar HEAD. The committed libraries moved to `Surface::Files`,
  the game calls `read_library` and `create_material`, and the preview lost its dropped writer hook.
- A new crate, `veyra-materials`, holds every `study:*` and `metro:*` key the kits bind.
  Each is either an ashlar stdlib graph with our parameters or a constant.
  The noise-texture generator, its PNGs and both hand-kept `materials.ron` files are deleted.
- `tools/content` (`just content`) bakes those materials and the two fixture buildings into `generated/`,
  which the game mounts as `generated://`.
  - A cold bake takes 25 s for 12 material sets.
  - A warm bake takes 26 ms and writes nothing.
  - Two cold runs wrote the same bytes to all 52 files.
- ashlar gained two things (ashlar CI green):
  - a library resolves its keys in its own asset source;
  - `Content::cache`, keyed by BLAKE3 per material set and per building, with byte-identical output.
    A write whose bytes are already on disk is skipped.
- The grounding fixture reads its proxies from the baked file and spawns through `AshlarPlugin`.
  - `game` links no kernel, graph engine or content crate, and `just server-check` asserts it.
  - Three of the six grounding views were captured: flat overview for both styles, and the uneven ground view.
    They match the references in geometry; the finishes are now graph bakes.
- `just bench` works through the game's new `--bench-out` summary.
  The baseline is in `docs/perf/ledger.md`, with two carried misses:
  - grove p95 goes to Phase 9;
  - the flight's hitches go to Phase 9 for the foliage stalls, and its terrain p95 to Phase 7.
- `just review` renders the building galleries by day and night.
  `tools/content` writes `generated/stats/buildings.ron`, snapshots it, and tests a budget.
  The corporate tower has 473 pieces at its nearest level and 1038 over all three.
  Phase 11 owns that number before a city of towers goes into the world.

### Phase 7: terrain

Terrain is the stage every other asset sits on, and today it reads as smooth noise in two planar layers.
This phase changes its shape and its surface, and removes the photo textures.

Work in `strata-worldgen` (no Bevy, the correctly-rounded float rule applies):

1. **The erosion filter** (runevision, March 2026), ported with `libm` for its trigonometry.
   It evaluates per point, so the world stays a pure function of the seed.
   Its ridge and drainage output becomes a `Sample` field.
2. **Terrain attributes** on `Sample`: plan and profile curvature, topographic position at two radii,
   aspect, and a wetness term from the drainage output.
   Flora (Phase 10), materials and the macro colour map all read them.
3. **Slope from the blended relief** rather than from the winning biome only,
   so slope materials stop jumping at biome borders.

Work in the content crate and `strata-terrain`:

4. **Stylised terrain layers as graphs** in a new game crate, `veyra-materials`:
   grass, forest floor, sand, rock, snow, gravel, plus soil, cliff strata and scree.
   They are baked to KTX2 by `just content` and stacked into the texture arrays `strata-terrain` already samples.
   The Poly Haven sets and `ATTRIBUTION.md` are deleted.
5. **The terrain shader**: hex tiling in place of the current stochastic lattice, height-based layer blending,
   biplanar mapping above about 35 degrees of slope, and a third splat layer so triple points stop seaming.
6. **A macro colour map** derived from the seed per region (4 to 8 m per texel),
   which the terrain fades to with distance.
   Grass roots and tree tints read the same map from Phase 9 on,
   so it is the one colour source the stylised direction needs.

Verify:

- Worldgen snapshots change once, in the commit that ports the filter, and are reviewed as images:
  the worldmap height and hydration renders for seed 42, before and after.
- A test holds the per-sample cost under 1.5 µs (today 852 ns) and the determinism tests still pass.
- The four fixtures before and after, plus a mountain fixture from the flight path.
- `grep` finds no JPG under `assets/`.

Exit gate: terrain at most 1.2 ms at every fixture, no stretched texels on cliffs in the mountain fixture,
every section 3 gate green.

**Done 2026-09-26.** What landed, and where it differs from the list above:

- **Erosion.** `strata_worldgen::erosion` is our own implementation of the described technique.
  - It is continuous (the continuity tests pass) and deterministic.
  - Its trigonometry and kernel are polynomial.
  - Gully spacing is one world field; strength is set per biome in `Relief::erosion` (mountains 1.2, highlands 0.8, forest 0.4, tundra 0.3).
  - It steers by the coarse relief's slope, because the fine creases carved cliffs.
  - Each relief's gradient is now analytic, from our own simplex kernel: bit-identical values, and a derivative that is correct where the `noise` crate's is not.
  - Cost per sample: 1.46 µs in eroded mountains, under the 1.5 µs bound; 0.95 µs in lowland.
  - ADR 0006 has an amendment.
- **Attributes.** `Sample::landform` carries slope, aspect, hollow and the gully ridge; drainage raises hydration.
  - Curvature and a second topographic radius were not added: the ridge and the hollow cover what Phase 10's rules name.
- **Slope** comes from the blended relief.
- **Terrain layers.**
  - Six stylised graphs in `veyra-materials` (`veyra:terrain-*`) are baked at 1024 by `just content`.
  - The terrain loads the baked KTX2 maps from `generated://` and packs roughness into the colour array's alpha and height into the normal array's alpha.
  - Soil, cliff strata and scree are deferred to Phase 10, where the geology rules first use them.
  - The Poly Haven terrain sets are deleted. The two bark sets stay until Phase 9 replaces the trees, so "no JPG under `assets/`" moves to Phase 9's checks.
- **Shader.**
  - Height-based blending between the biome pair and into rock, so rock breaks through along its plates rather than along triangles.
  - A quarter-turn rotation per stochastic lattice corner, and height-weighted corner blending (the part of hex tiling that matters here). The existing triangular lattice was kept.
  - Biplanar rock on steep faces in world-stable coordinates.
  - The third splat layer was not built: triple points keep the accepted seam `mesh.rs` documents.
- **Macro colour.** It became a per-vertex landform term in the far-field tint (ridges lighter, gullies darker) rather than a separate texture map. It is one colour source and costs nothing, and the per-region map can come later if the far field needs finer detail than vertices give.
- **Streaming.** Terrain mesh inserts are budgeted at 1 MiB a frame. On the same day this took the flight without foliage from p95 9.5 ms to 7.6 ms, which pays back the terrain share of the carried miss.
- **Verification.**
  - `just ci` is green.
  - The worldgen and flora snapshots were reviewed and re-accepted.
  - Worldmap and in-game captures of the eroded mountains were reviewed.
  - The full bench was not recorded: runs on this machine are paced unpredictably, and the user chose to skip gating for now (`docs/perf/ledger.md` has what was measured).
  - The woodland fixture's p95 rose to about 8.5 ms from CPU cost per entity, with 5% more plants from drainage. It is carried to Phase 9 with grove.
- **Known costs.**
  - Any change to the graph library invalidates every material in the content cache (a 60 s rebake). Hashing only the graphs a set reaches is the fix, when it starts to hurt.

### Phase 8: ashlar organic foundations

Trees and rocks need things ashlar does not have.
These are all things any ashlar game would want, so they go into ashlar, with tests and preview scenes there.

Work in ashlar:

1. **Alpha and transmission.** An opacity output on `PbrOutput`, a thickness plane,
   an alpha mode and transmission on `MaterialDefinition`, and the matching arms in `create_material`.
   This reopens what ashlar ADR 0003 put out of scope, so it gets an amendment there.
2. **Non-square bakes and atlases.** Bakes at any power-of-two aspect ratio, and a packer (`etagere`)
   for leaf, twig and impostor sheets.
3. **Vertex data.** Vertex colours and named extra attributes on `TriangleMesh` and in the baked format,
   uploaded as Bevy mesh attributes.
4. **A mesh-set asset kind.** Levels, colliders and attributes without the building vocabulary of parts and rooms,
   loaded and spawned by `AshlarPlugin` with a `VisibilityRange` per level, and readable with plain `ashlar`.
   This is where trees and rocks live.
5. **The impostor baker.** Hemi-octahedral frames (12 by 12 to start) of albedo with alpha and normal with depth,
   CPU ray-cast with `obvhs` so CI gets identical bytes.
   The runtime is a material in `ashlar-bevy` that blends three frames with a depth parallax.

Work in the game:

6. A throwaway `veyra-flora` stub that bakes one tree from today's `build_tree` output through the new asset kind,
   so the whole path runs end to end before Phase 9 replaces the generator.

Verify:

- ashlar's tests bake twice to identical bytes, alpha round-trips through KTX2, and the preview shows cut-out cards.
- The stub tree appears in the game from `generated://`, with its impostor at the far band.
- **The capacity test, which is this phase's reason to exist.** 20 species by 8 variants by 4 levels of a synthetic 15k tree:
  load time, GPU memory, upload per frame, and frame time at the woodland fixture density with near shadows on.
  This is where we find out whether the 15k cap and the 60 m near band hold.
- **The impostor test**: no visible switch in a still at the transition under MSAA 4x and under TAA,
  and Bevy issue #17643 (dither artifacts with TAA) checked on 0.19.

Exit gate: the capacity test inside the tree row of the frame budget.
If it fails, we shrink the near band or the cap here, before any tree is authored against numbers that do not hold.

**Done 2026-09-26.** ashlar ADR 0007 records the design, and three branches of ashlar work merged into its main:

- **Materials.** `Alpha::Mask`, `double_sided` and `diffuse_transmission` on `MaterialDefinition`, and an `opacity` output baked into the base colour's alpha.
  - The mip chain preserves coverage.
  - Opacity is refused in compiled `Shader` surfaces.
- **Props.**
  - `TriangleMesh` carries colours and named vertex data (`VertexData`). `ashlar_bevy::attribute` gives a game the same Bevy attribute by name.
  - `ashlar::prop::Prop`: levels, colliders and an impostor last level. It is a `.ashprop` file readable with plain `ashlar`, written by `Content::write_prop` and loaded by `AshlarPlugin` with shared meshes.
  - `AshlarProp` spawns one, and `prop_level_range` gives a game that manages its own entities the same bands.
- **Impostors.**
  - `ashlar-impostor` bakes a hemi-octahedral atlas on the CPU: albedo with alpha, normal with depth, and the same bytes every run.
  - `ashlar_bevy::impostor` draws it as a camera-facing quad blending three frames, lit by Bevy's standard lighting.
  - `Content::write_impostor` bakes one from a prop's nearest level and its material graphs.

Non-square bakes and an atlas packer were not needed: impostor and leaf sheets are square grids.
The stub tree through `build_tree` was not built either. The capacity fixture already drives the full path: content step, `.ashprop` file, loader, shared meshes, bands and impostors.

The capacity test is `just content --capacity`, then `prop-capacity`: 20 species by 8 variants of a synthetic tree at about 14 900, 5 000 and 1 400 triangles, with colour and wind data on every vertex and cut-out, two-sided, translucent leaf cards. Measured once each, in unpaced runs:

- **Out to 350 m** (every mesh level, 4 768 trees as 28 608 entities): median frame 3.6 ms, GPU 2.4 ms against the 2.5 ms tree budget, near shadows included. So the 15k cap and the 60 m near band hold, with no room to spare.
- **Out to 1.6 km** with an impostor per species beyond 350 m (99 300 trees as 695 100 entities): median frame 6.9 ms, GPU 2.8 ms. The frame is CPU-bound on entities.

Findings Phase 9 must act on:

1. **Far trees cannot be an entity each.** Seven entities per tree is 700 000 at forest range, and entities are also what the grove and woodland misses carried here are made of. Phase 9 draws impostors, and probably the coarsest mesh level, as instanced batches per cell (a storage buffer of instances, as grass is drawn). Only trees near enough for mesh levels are entities, and only for the levels their cell's distance can reach.
2. **Mesh memory.** 160 variants hold 443 MB of mesh at 64 bytes per vertex. Quantised normals, colour and wind would halve it, and fewer variants per species would cut it further.
3. **Texture memory.** A 12 x 12 atlas of 128 px frames is about 19 MB per variant uncompressed. Atlases are per species at 64 px here, and Phase 9 needs block compression (BC7 and BC5) in the content step before leaf and impostor sheets multiply.

### Phase 9: trees

The phase with the biggest visual change.
The runtime tree generator is replaced by a game-side generator in the content step,
and the world starts depending on a baked library.

Work in `veyra-flora` (no Bevy):

1. **Growth.** Space colonisation inside a species' crown envelope, pipe-model radii computed bottom-up,
   gravity bending accumulated along the branch, shade pruning into dead stubs.
   Weber and Penn remains for conifers.
   Alien axes (topology, tropism targets, phyllotaxis angle, cross-section) exist as parameters with no alien species using them.
2. **Meshing.** Parallel-transport tubes with ring counts by radius, collar flares, a buttress term at the root,
   and bark UVs with an integer repeat around each ring.
   Tubes are meshed directly into `TriangleMesh`; the kernel is not involved.
3. **Foliage.** 4 to 8 twig variants per species modelled as geometry, baked to a twig atlas
   (albedo, alpha, normal, thickness, AO), then placed as cluster cards on terminal shoots.
   Card normals take 80% or more from the crown envelope.
4. **Stylised data.** Vertex AO, normalised height and a curvature term on every vertex,
   so the painterly shaders have their inputs when they come.
5. **Wind data.** A pivot index and hierarchy level per vertex, and a per-variant pivot table.
   `strata-foliage`'s tree shader walks up to three levels, and so do the prepass and shadow passes.
6. **Levels.** At most 15k triangles, then about 5k, about 1.5k, then the Phase 8 impostor.
   Only the nearest level casts shadows, as the foliage spike settled.
7. **Species.** Today's oak, pine and scrub rebuilt, plus at least two more so each forest biome mixes three or more.
   Bark and leaf graphs live in `veyra-materials`.

Work across the game:

8. `strata-flora` placement picks a species and variant from the library instead of generating.
   Trunk colliders come from the library's collider table, on the server as on the client.
9. **ADR 0005 amendment**: the world is the seed plus a versioned library.
   The library version joins the protocol handshake, so a client with another library is refused.
10. `build_tree`, `leaf_cluster_image` and the startup impostor baker are deleted.

Verify:

- Stats tests: every variant at most 15k triangles at LOD0, pipe-model ratios within bounds, every card on wood.
- `just review`: a turntable and a 64 px silhouette sheet of 16 variants per species, next to today's trees.
  If two variants cannot be told apart at 64 px, the variation is too small.
- The end-to-end check with two clients: both walk into the same trunk at the same place.
- A client with a different library version is refused with a message naming both versions.

Exit gate: trees within their 2.5 ms at the woodland fixture, no pop in a still at any level switch,
every section 3 gate green.

**Done 2026-09-27.** The world is now the seed plus a baked tree library (ADR 0005's 2026-09-26 amendment).

- **Generator.** `veyra-flora` grows oak, birch and scrub by space colonisation inside a crown envelope, and pine and fir from whorls on a leader, with pipe-model radii, gravity droop, dead stubs, parallel-transport tubes and integer bark repeats.
  - Five species, six variants each: four mature, one young, one old.
  - Every vertex carries wind data (pivot, level, flutter, phase) and shade data (occlusion, height, curvature, crown depth).
  - Each variant has a pivot table and one trunk capsule.
- **Materials.** `veyra-materials` gains five bark graphs and five painted leaf-cluster cards (cut-out, two-sided, translucent).
- **Content step.** `just content` grows every variant and bakes it to `generated/flora/<species>-<variant>.ashprop`: three mesh levels at 60, 150 and 350 m (40, 100 and 200 m for scrub), and a hemi-octahedral impostor per species.
  - It writes `generated/flora/library.ron`, versioned by a hash of its contents.
  - 30 variants, 342 300 near triangles in all, 70 MB.
- **Placement and collision.** Placement takes a baked species' variant count and trunks from the library, on the server as on the client.
  - The library's version joins the replicon protocol hash, so a client with another library is refused. The message names no versions, only the protocol mismatch.
  - *(2026-09-29, ticket 0004: since its step 1 a refused server sends its versions and the client says in words which differs: trees, world or seed.)*
- **Runtime.**
  - Near trees are entities per mesh level with a `TreeMaterial`, the ashlar material extended with the variant's pivot table. The wind shader walks from a vertex's pivot to the trunk, weighting each turn where the child attaches, in the main, prepass and shadow passes.
  - Far trees are one impostor batch per species per cell: a small mesh whose vertices carry the trees, under one shared material per species. Each tree fades in on its own distance with the dither its mesh level fades out with.
  - The understory is converted into the same vertex contract and wears the baked bark and leaves.
- **Deleted:** the runtime tree path of `build_tree` (now `build_understory`), the procedural leaf texture, the startup impostor baker, and the photographed bark under `assets/textures/`. A test keeps JPGs out of `assets/`.
- **Species.** Birch and fir join the registry, so the forest mixes four tree species. The forest weights were cut so the total stays near the old one.

What measurement found and fixed along the way:

- **A storage buffer per far batch** made a material per batch: 12 000 draws, 60 ms of CPU in the woodland fixture. Moving the trees into the batch mesh's vertices brought it to about 2 ms.
- **The understory's 180 m reach** was set on level 0, so every tree's 15k-triangle level was prepared out to 308 m. The understory now rides in level slot 1.
- **The carried flight miss.** It was not the foliage's collection: two things blocked the frame.
  - The cell scheduler walked all 2 300 wanted cells every frame through a linear cache search: 5.7 ms at the median. It now stops as soon as no job can start, and caches by key.
  - Avian optimised its collider trees on the async compute pool, which terrain meshing and foliage placement fill, then blocked the tick on the result for up to 80 ms. It now optimises in place.
  - Flight went from 240 frames over 33 ms and a p95 of 28.8 ms to 1 frame and 9.05 ms.
- **Woodland.** Settled after 3 000 frames on the same machine and day: 13.7 ms against 12.6 ms for the Phase 8 build, with GPU equal (8.6 against 8.5 ms). That is 17% more plants and 15k-triangle trees, drawn with 64 700 entities against 161 800. Its p95 of 12.3 ms is still over the gate: the frame is CPU-bound on entities. The bench's scene is streaming for most of its run.

Deferred, with reasons:

- **Block compression and quantised vertices.** Atlases are uncompressed at 1 536 texels per species, and meshes are 64 bytes a vertex.
- **An impostor per variant.** One per species, scaled to each variant's height, until compression makes six affordable.
- **Twig atlases modelled as geometry** (item 3). Leaf cards are painted clusters from material graphs.
- **The 16-variant review sheet.** `veyra-flora`'s ignored review test writes six variants per species, silhouettes and level comparisons into `scratch/trees/`.
- **"No pop at any level switch" as a measured gate.** Checked by eye in stills only.
- **A test binary crash on this machine.** `strata-flora`'s placement tests segfault intermittently, in debug builds only and only with many threads. It is the same at the Phase 8 commit and with Rust 1.96. Valgrind and AddressSanitizer find nothing, and the code has no `unsafe`. A determinism test once failed at a case that passes on every rerun. Together that looks like the i9-13900KF instability known in that CPU family, not a code bug.

### Phase 10: scatter, understory, rocks and cliffs

Better trees on a jittered grid still look planted.
This phase changes where things go, and adds the rocks the world has never had.

Work in `strata-flora`:

1. **Clustered placement.** Thomas-process parents hashed per 32 m cell with a halo,
   multiplied by a per-species low-frequency patch field.
   Groves, thickets, gaps and clearings come from this.
2. **Size follows fit**, so forest edges taper, and an edge band favours shrubs and saplings.
3. **Parent and child scatter.** Trees spawn saplings, deadfall and ferns, rocks spawn moss, pebbles and ferns,
   each child hashed from its parent's id.
4. **Attribute-driven fit.** Species read curvature, topographic position, aspect and wetness from Phase 7.
5. **Geology placement.** Outcrops at convex slope breaks, scree fans below steep slopes,
   boulders in drainage lines, cliff pieces sunk into slopes above about 45 degrees.
   Rocks get colliders on both ends.
6. **Grass.** Voronoi clumps with shared lean, height and colour, roots coloured from the macro map,
   rounded normals, and flower patches.

Work in ashlar:

7. **Organic shapes in `Geometry`.** SDF level sets, smoothing, warps and sweeps through the Manifold kernel,
   with displacement described as data, and `LodPolicy` simplification for hulls and level sets.
   The `kernel_init` canary runs first, because ashlar ADR 0005 recorded crashes in exactly these kernel calls.

Work in `veyra-rocks` (no Bevy):

8. Rock families per geology (at least two), 8 or more rocks each, and a cliff kit of 3 or more pieces.
   They carry the same stylised vertex data as trees, and a hull collider each.
   Moss on their tops is ashlar strands near the camera and a world-space mask beyond,
   using a precompiled material extension so the game needs no tool feature.
9. The understory forms (flowers, ferns, shrubs, logs) move into `veyra-flora` and the library.
   Logs gain colliders.

Verify:

- A placement statistics test: the pair-correlation of tree placements over 64 cells shows clustering,
  where the old placement is near-uniform; the test records both.
- Worldmap cover renders and all fixtures, before and after.
- Server agreement for rock and log colliders, and the end-to-end check.
- Placement stays under 3 ms per cell with the extra rules.

Exit gate: rocks within 0.6 ms and grass and understory within 1.2 ms at their busiest fixtures,
server tick within its gate with rock and log colliders, every section 3 gate green.
At this point the refactor's definition in section 1 holds, and we check it item by item.

**Done 2026-09-27.** ADR 0005 records the placement rules (its 2026-09-27 amendment).

- **Rocks.** `veyra-rocks` shapes granite and sandstone boulders, stones and a cliff kit (wall, buttress, ledge) from signed distance fields.
  - It meshes them with its own surface-nets mesher with dual-contouring vertices, not the kernel. The plan's fallback was taken, because the Manifold level sets had crashed before.
  - Three levels each, with box-projected metre UVs (`v` down a wall), stylised shade data and moss in vertex colour.
  - Convex hull colliders: one per rock and four per cliff piece. Each holds every vertex of the nearest level within 10 cm.
  - Six species of eight variants. `veyra-materials` gains granite and sandstone graphs.
- **Understory.** Ferns, two wildflowers, the woodland shrub and a fallen log are baked by `veyra-flora` into the library, like trees. Flowers are coloured by their vertices on a plain surface. The log has a capsule collider.
- **The library's collider** is now general: a cylinder, a capsule, convex hulls or none. The server builds it at the plant's root, turned by its yaw.
- **Placement.**
  - Rocks are a flora kind, placed first. They read slope, hollows and ridges from the landform, and cliff pieces face down the slope.
  - Trees and rocks gather into clumps with clearings between them. Trees' size follows fit, and trees and boulders bring children: ferns under oaks and birches, stones round boulders and below cliffs.
  - The forest's pair correlation within 20 m went from 0.94 to 1.35, measured by a test.
  - Grass tufts share their shade and round their normals over the tuft.
- **Runtime.** The tree material binds vertex colour and hands it to Bevy's own input, and a plain surface is not brightened like a painted map.

Measured once each, unpaced: flight p95 9.26 ms with one frame over 33 ms; woodland p50 10.22 ms and p95 12.67 ms, at 48 188 entities. Both are as at the end of Phase 9.

Deferred, with reasons:

- **Placement cost.** About 21 ms of a worker thread per cell, against the 3 ms the plan asked; it was 15 ms before this phase.
  - Most of it is sampling the neighbours' halo strips, which grew with the cliffs' exclusion.
  - The fix is to sample the halo only where candidates fall. It is a refactor of `HeightGrid` for the next pass. Placement runs off the frame, and the flight fixture shows no stall from it.
- **Organic shapes in ashlar's `Geometry`** (item 7). The rocks did not need the kernel. SDFs in ashlar can come when a building needs them.
- **Moss as strands** near the camera. Moss is vertex colour on the rock.
- **Flower patches in the grass.** The wildflowers are placed understory with their own patch field.
- **Roots coloured from the macro map.**
- **Cliff pieces placed in lines along a slope band.** Their spacing grid shows where a steep band runs straight; an art pass item for Phase 12.

### Phase 11: the settlement

The world gets its first city.
The hard part is not the buildings, which ashlar already bakes, but fitting a city into a procedural world that a server must agree on.

Work in `strata-worldgen` and `strata-sim`:

1. **A settlement site.** A seeded site near the default spawn, chosen by rules
   (low slope, dry land, near the coast in keeping with Port Serein),
   with a stable id and a footprint.
   A registry entry names it, so later settlements are data.
2. **Terrain that receives it.** Inside the footprint, `sample()` blends the relief towards a terraced platform,
   with a smooth falloff outside it.
   It stays a pure function of the seed, so the server's heightfield agrees with no extra data.
   This deliberately differs from the grounding study, which left worldgen alone:
   a city is too large to stand on foundations and ramps.
3. **A road** from the site edge into the wilderness: a spline over the terrain, which flattens a strip in `sample()`,
   feeds a road layer to the terrain material, and clears vegetation through the existing footprints.

Work in the content crate (a new `veyra-settlements`, no Bevy):

4. **The core**: `ashlar_showcase::city::metropolis` at about 4 by 4 blocks (about 240 m across), baked by `just content`.
   Its dark, stepped towers are close to the setting's Pelagic districts, so we start from the example
   and restyle it with our own materials rather than write a city kit first.
5. **The ring**: `veyra-buildings`' outposts, the corporate annex and `ashlar-showcase`'s sci-fi habitats
   placed around the core by the same site rules, each fitted with `fit_ground` at its lot.

Work in the game:

6. **City streaming.** Pieces grouped by 40 m ground cell, level band and side, disabled out of range with hysteresis,
   after ashlar's web example.
   Interiors are hidden beyond 40 m and never cast shadows.
7. **Server colliders.** `strata-physics` reads the baked buildings' collider solids with plain `ashlar`
   and builds static colliders around each player, like trunks.
   `ashlar-bevy` stays render-side; this is why Phase 8's asset kind must be readable without Bevy.
8. **City lighting.** Emissive windows and street lights read from the baked materials, and a night check.

Verify:

- Determinism: the site, terrain and road are identical from the same seed on client and server,
  and the worldgen snapshots are re-accepted with the site in them.
- The character walks from the forest along the road into the city, down a street and into a lobby,
  without falling through or catching on seams; a scripted walk does the same in CI with `--headless`.
- The end-to-end check in the city: two clients, same buildings, same collisions.
- Street level, rooftop and approach fixtures by day and by night.

Exit gate: the three city fixtures inside the gates, with city and buildings at most 2.0 ms,
server tick within its gate with both players in the city, no streaming hitch on the approach.

**Done 2026-09-27.** ADR 0009 records the design.

- **Site.** `assets/settlements/port_serein.settlement.ron` names the search. The world chooses the flattest dry ground near the origin: (-2550, -1350) on seed 42, its floor 113.7 m up, in 22 ms at world creation.
- **Terrain.** `sample()` blends a platform (radius 180 m, falloff 90 m) 0.3 m under the city floor, and a traced 1.2 km gravel road leaving north from the core street.
  - The landform's `developed` keeps every plant and rock off both, on the server as on the client.
- **City.** `veyra-settlements` lays Port Serein from `ashlar-showcase`'s metropolis at 3 by 3 blocks, 192 m across, restyled with Pelagic materials.
  - It was cut from 42 816 placed elements to 15 894 by capping tower heights, dropping upper interiors and fusing trim.
  - Around it sit a paved promenade, six outposts, the corporate annex and three habitats: 23 buildings, 16 993 elements, 19 242 nearest-level pieces.
  - The content step bakes them and writes the layout, `generated/settlements/port_serein.ron`.
- **Collision.** The physics reads each baked building's proxies with plain `ashlar` and builds one static compound per building, plus a floor disc at street level. On the server as on the client.
- **Client.** The client spawns the same baked files. `--at city` and `--at road` start a player there, in the game and on the server.

Verified:

- The same seed gives the same site: a worldgen test, and server and client logs agree.
- Worldgen snapshots are unchanged apart from the new `developed` field; the site is away from their sample points.
- A walker spawned on the street walks 19.5 m into the landmark's lobby and stops at the reception desk, grounded.
- End to end: a server with `--at city` and two walking clients, both grounded in the city. One is in the lobby, the other 68 m down a street. Server tick 0.43 ms mean, 0.82 ms worst.
- Street view 87 fps (11.6 ms, 5.4 ms GPU); lobby 68 fps (14.6 ms); above the city at night 99 fps. Emissive windows and neon read at night.

Deferred, with reasons:

- **City piece streaming** (item 6): grouping by 40 m cell and disabling out of range. The city's pieces are entities under their visibility ranges, about 43 000 of them; the frame stays under 15 ms in the city. Next in line for the CPU budget.
- **The settlement layout is not in the replication handshake.** The tree library is; the content step should version all its output together.
- **The walk from the forest along the road into the city,** as one scripted CI run. The pieces are checked separately: the road exists and is graded, and the street-to-lobby walk works.
- **Street, rooftop and approach fixtures by day and night** in `tools/bench.py`, and the night atmosphere, which is very dark away from the emissives. Phase 12.
- **The site's coastal preference** did not find a coast within the search. The city stands in dry country.

### Phase 12: slice polish and acceptance

The pieces all exist; this phase makes them read as one place, and proves section 1 holds.

Work:

1. **An art pass driven by the review sheets.** Palette per biome through the macro colour map,
   species mix, rock density, road edges, city materials.
   Only data and graph parameters change here, never systems.
2. **The route.** A scripted walk and a scripted flight along it:
   spawn in the forest, down the road, through the city, back out.
   Both become permanent bench fixtures.
3. **Documents.** PLAN.md records Phases 6 to 12 as done with their measured numbers,
   ADR 0005's amendment is final, a new ADR records settlement sites in worldgen,
   CLAUDE.md learns `just content`, `just review`, `just bench` and the `generated://` source,
   and README says how to run the slice.

Verify:

- Section 1's two definitions, item by item, with the evidence for each written into PLAN.md.
- The full ledger, every fixture, every gate.
- The end-to-end check along the whole route with two clients.

Exit gate: both definitions in section 1 hold, and the ledger shows every gate green on the day we call it done.

**Done 2026-09-27.** Section 1, item by item, with the evidence:

**The refactor is finished.**

- **No photo textures remain in `assets/`.** Terrain layers, bark, leaves and rock surfaces are graph bakes. A test (`tools/content/tests/trees.rs`) keeps JPGs out.
- **`build_tree`'s tree path, the startup leaf images and the startup impostor baker are gone.** Trees, understory and rocks load from the baked library. `build_understory` remains only as the fallback for a world with no content step.
- **Neither `game` nor `server` links the kernel, the graph engine, `ashlar-content`, `ashlar-impostor` or any `veyra-*` generator.** `just server-check` asserts it.
- **The server derives every collider from baked data.** Trees, logs and rocks come from the library, which the content step derived from the props. Buildings come from the baked `.ashlar` files, read with plain `ashlar`.
- **`just content` is deterministic.** Two cold runs with no cache wrote identical bytes (204 s each), and a warm run took 3 s and rewrote nothing.

**The first slice is passable.**

- **Site.** A seeded site 2.9 km from the origin, on terrain shaped to receive it, with a dense core of ashlar city-kit towers and a Veyra ring. Phase 11.
- **Road.** One road from the wilderness into the city, with vegetation and rocks cleared from it and from the platform.
- **The way in.** Forests in groves with clearings, understory, rocks and cliffs on the way, stylised and organic by the review sheets and the captures, with no grid showing.
  - Cliffs on a straight steep band stood in a row; they are now clumped too.
  - "No pop at a level switch" is judged by eye in stills. It is not a measured test.
- **The route.** `--route` walks the road in, the street, the landmark's lobby and back out: all 101 waypoints, grounded throughout, headless.
- **Multiplayer.** Two clients on one server see the same city and collide with the same buildings, trees and rocks. The city end to end is in Phase 11; trees and rocks share the placement and the library.
- **Performance gates.** Held at the valley, coast, walk and all three city fixtures.
  - Missed at woodland and grove (p95 12.3 to 13.1 ms), CPU-bound on foliage entities. The miss was carried from the Phase 6 baseline and is no worse at Phase 9's same-machine comparison.
  - Missed at the flight's p95 (9.6 ms), with no frame over 33 ms.
  - Route-walk's p95 is 8.5 ms, with one frame of 41 ms while the city's buildings arrive.
  - Placement per cell, about 21 ms on a worker thread, misses its 3 ms gate.

So the slice is passable to walk, look at and play two-player, and not yet inside every frame-time gate. The misses that remain are named, with their owners below.

Work in this phase:

- `--route`, the route as a scripted walk, and the route-walk, city-street, city-rooftop and city-approach bench fixtures.
- Settlement streaming: buildings in within 2.5 km and out past 3 km, one a frame.
- The road meets the platform at its height, which had left a lip the walker could not cross (ADR 0009).
- Cliffs clumped. The README says how to run the slice.

Carried, with owners:

1. **Foliage entities at the dense forest fixtures** ([ticket 0001](tickets/0001-foliage-entity-cost.md)). The next step is the one Phase 8 named for far trees: draw the coarsest mesh level as instanced batches too, and understory the same way.
2. **Placement per cell.** Sample the neighbour halo only where candidates fall.
3. **City piece streaming by 40 m cell and band,** and the settlement layout in the replication handshake.
4. **Block compression and vertex quantisation** for atlases and meshes.
5. **The night atmosphere,** very dark away from the emissives, and the painterly shading, which is out of this plan's scope.

## 5. Risks we know about

- **15k trees may not fit.** The only shadow measurement we have was taken with 2600-triangle trees.
  Phase 8's capacity test exists to find this out before Phase 9, not after.
- **Manifold level sets and smoothing crashed before** (ashlar ADR 0005, SIGFPE under an Anaconda GCC).
  Phase 10 runs the canary first and falls back to `fast-surface-nets` for rocks if the kernel is unreliable.
- **VisibilityRange dither under TAA** (Bevy #17643). If it is still there, trees keep MSAA until it is fixed.
- **The erosion filter's cost.** Worldgen runs per sample on client and server.
  If the port pushes a sample past 1.5 µs, we evaluate the filter on the coarse grid only and interpolate.
- **A city in a pure-function world.** Terracing inside `sample()` must stay cheap for samples outside every site.
  A coarse site index keeps the common case to one lookup.
- **Library size.** 20 species by 8 variants with atlases and impostors might be large.
  The stats files track it from Phase 8 on, and the ledger gets a download-size row.
