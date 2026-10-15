# Asset pipeline discovery: organic assets on ashlar

2026-09-26. Discovery for two goals.
First, move asset generation onto the ashlar stack,
so generated content comes from a content step instead of from Poly Haven downloads and startup code.
Second, make trees, scatter and terrain read as organic and AAA rather than procedural.

This document records what we have, what ashlar can and cannot do today,
what the industry does, and a proposed shape with spikes to de-risk it.

## Decisions (2026-09-26)

These answer the first round of open questions, and sections 3 to 5 are written against them.

1. **Generators belong to the game; ashlar is our library.** ashlar is not part of the game,
   so the tree, plant, rock and terrain-layer generators live in this repository, beside `veyra-buildings`.
   ashlar is still our own crate system, and we add to it or change it whenever we find a gap.
   Its example content, `ashlar-showcase` and the stdlib materials, may be used freely.
2. **Anything may change.** Every seed, snapshot and the worldmap can move.
   Placement, erosion and the species library are all on the table.
3. **High-quality stylised.** Painterly soft shading and stylised lighting in the spirit of Guild Wars come later.
   The assets have to carry the data those shaders will need (section 3, "Built for a stylised look").
4. **The alien share is open.** The generators keep alien axes available as parameters,
   but no alien species or biome is committed yet.
5. **Fully procedural now.** No scans and no AI exemplars.
   The Poly Haven terrain and bark sets are replaced by graph bakes,
   and the TRELLIS workflow in `docs/SETTING.md` is set aside.
6. **Trees cap at about 15k triangles** at the nearest level, with a level-of-detail chain below it.

## 1. Where we stand

### The game today

Every asset is either a CC0 photo set on disk or Rust that runs at startup or at runtime.
There is no offline pipeline.

| Asset | Source today | Where |
| --- | --- | --- |
| Terrain layers (6) | Poly Haven 2k JPGs, fetched by hand, stacked into arrays at startup | `assets/textures/`, `strata-terrain/src/material.rs` |
| Bark (2) | Poly Haven 1k JPGs, base colour multiplied by 2.2, roughness map unused | `assets/textures/bark_*` |
| Tree meshes | Weber-Penn-like recursion at startup, 3 LODs, under 2600 triangles | `strata-flora/src/tree.rs` |
| Understory (4 forms) | Rust meshes at startup, solid colours | `strata-flora/src/tree/understory.rs` |
| Leaf cards (2) | 256 px grey images drawn at startup, shared by every species | `strata-foliage/src/tree.rs` `leaf_cluster_image` |
| Impostors | CPU-rasterised at startup, 8 azimuths by 128 px, nearest view, no elevation | `strata-foliage/src/impostor.rs` |
| Grass | Per-cell storage buffers, untextured blades | `strata-foliage/src/grass.rs` |
| Building meshes | Manifold kernel runs inside `game` at startup | `apps/game/src/grounding.rs` |
| Building textures | Rust noise written to PNG by the preview tool | `veyra-buildings/src/textures.rs` |
| Rocks, cliffs, props | None | |

Vegetation is 2 trees, 1 bush, 4 understory forms and 2 grasses.
Desert, tundra, mountains, savanna and coast have almost nothing distinctive growing.

Placement in `strata-flora` is sound as a base.
Each species sits on its own jittered grid, a hash threshold keeps slots by fit,
and Matérn type-II thinning with a halo resolves conflicts across cell borders.
It is deterministic and the server derives the same trunk colliders from it.
It looks procedural for the reasons you can see in `docs/research/tree-organic.png`:

- One candidate per stratum gives near-uniform spacing with a hard ceiling of 49 oaks per cell.
- Density comes only from smooth fields, so there are no groves, thickets, gaps or clearings.
  Trees get no mid-scale patch noise at all; only the 600 m cover field varies them.
- Scale is uniform random and does not follow fit, so forest edges do not taper.
- There is no parent and child scatter: no ferns under canopy, no shrubs at edges, no saplings near parents.
- Crowns share one leaf image and one tint per species, and `tint_dry` is never read for trees.
- Tubes are 6 and 3 sided with unwelded joints, and wind sways the whole tree as one rigid body.
- The terrain is two-layer planar splat with no height blend, no triplanar, no macro normal
  and no erosion, so mountains read as smooth noise in flat white.

### The build is broken right now

The uncommitted migration does not compile against ashlar HEAD (`e824b8d`).
`cargo check --workspace --all-targets` stops at three errors:

```
error[E0560]: struct `MaterialDefinition` has no field named `base_color_texture`
   --> crates/veyra-buildings/src/textures.rs:128:9
(and `normal_texture`, `orm_texture` on the next two lines)
```

The inventory also found `ashlar_bevy::read` and `ashlar_bevy::create`,
which are `read_library` and `create_material` at HEAD,
and `ashlar_preview::Catalog::asset_writer`, which does not exist any more.
The check stops at the first crate, so those are unconfirmed.
This has to be fixed before any spike builds on the migration.

### What ashlar gives us

ashlar is a building system with a material engine attached.
Here is what carries over to organic work:

- **The content step split.** `ashlar-content` links the Manifold kernel and the graph engine,
  writes `.ashlar` files and zstd KTX2 maps, and the game links `ashlar-bevy` with no kernel.
  This is the model we want for every generated asset, not just buildings.
- **The material graph.** About 70 node types: value and Perlin noise, Voronoi with edge and cell outputs,
  warps, slope blur, curvature, horizon AO from height, jump-flood distance, 2D SDF shapes.
  It bakes tileable base colour, normal, ORM, height and emissive on the CPU or GPU,
  with bytes that do not depend on the thread split.
  Bark, rock faces, cliff strata, soil and forest floor are all expressible with what is there.
- **Strands.** Grass, moss and fur as ranked strand sets per UV repeat,
  grown on any triangle mesh with metre UVs, with rank-prefix LODs, crossed-quad cards and a wind material.
- **`BakedBuilding` as a container.** `BakedBuilding::new(building, levels, colliders, portals)`
  accepts arbitrary `TriangleMesh` levels, and `AshlarPlugin` spawns them with a `VisibilityRange` per level.
  `check_against_recipe` only checks ids, so a generator can hand it meshes that never went through the kernel.
- **The preview `Catalog`**, which takes game scenes, and `Content::write_file` for new file kinds.

It also has hard gaps for organic assets:

- **Geometry.** There is no sweep or tube along a curve, no SDF, no smooth union, no warp and no displacement.
  The Manifold kernel has `from_sdf`, `warp`, `smooth_out`, `refine` and `minkowski_sum`; ashlar exposes none of them.
  Hulls are never simplified by `LodPolicy`, so a hull rock gets no LOD.
- **Mesh data.** `TriangleMesh` has positions, normals and metre UVs only.
  There are no vertex colours, no second UV set and no custom attributes,
  so wind hierarchy, AO and per-branch data have nowhere to live.
- **Materials are opaque and square.** `PbrOutput` has no opacity, there is no thickness or transmission map,
  bakes are square powers of two, and ADR 0003 puts atlases and transparency out of scope.
  A leaf card cannot be expressed today.
- **Runtime world-space nodes** (`Triplanar`, `WorldMask`) exist only behind the `shader` tool feature,
  so moss on rock tops is not available to a shipped game.
- **No scatter, instancing, impostors or decals.** Mesh sharing is per part in `AshlarCache`.
- **Strands at world scale.** A two-metre grass repeat at full detail is about 690k triangles,
  and strand sets are 11 to 24 MiB uncompressed.
  That fits a lawn or a moss carpet on a rock, not 6.8 million blades of open-world grass.

## 2. What the industry does

The three research reports behind this section are long; this is what we take from them.
Every claim has a reference in the report it came from, and the key ones are listed in section 7.

### Trees and plants

The tools that make trees look natural share one pipeline shape,
and Unreal's Procedural Vegetation Editor (5.7) states it most cleanly:
grow a dense skeleton, modify it, mesh it, distribute foliage, bake, reduce, export.

- **Growth.** Space colonisation (Runions 2007) inside an art-directed crown envelope
  gives the best silhouette for the least code.
  Self-organising trees (Palubicki 2009) add light competition and apical dominance,
  which make asymmetry and dead lower limbs emerge on their own.
  Weber and Penn stays right for conifers, palms and ferns,
  and L-systems for flowers, herbs and structured alien forms.
- **Botanical rules**, whatever the algorithm: pipe-model radii computed bottom-up
  (`r_parent^n = Σ r_child^n`, n about 2.5), golden-angle phyllotaxis,
  gravity bending accumulated per year so old limbs droop, shade pruning into dead stubs.
- **Meshing.** Parallel-transport tubes with ring counts by radius,
  a branch collar flare, and a buttress formula for the root flare.
  Manifold union with local smoothing for first and second order junctions at LOD0 only,
  which is authoring-time work and fits the kernel's place in the content step.
  Bark UVs use an integer repeat count around each ring, so the seam never shows.
- **Twig cluster cards are the single biggest visual step.**
  We model 4 to 8 twigs as real geometry, bake them to an atlas with albedo, alpha, normal,
  thickness and AO, then place cards on terminal shoots instead of single leaves.
  Blending card normals toward the crown envelope normal and adding canopy AO removes the flat-card look.
  That matters doubly for us, because diffuse transmission needs coherent normals on the back-lit side.
- **Wind** in the Pivot Painter style: a pivot index per vertex into a per-variant table of pivots,
  axes, stiffness and natural frequency, walked for up to three levels in the vertex shader.
  Our current tree shader dropped hierarchy because shearing tore joints;
  rotating rigidly about real attachment points is what fixes that.
- **LOD.** Full tree near, about 40% then about 10% triangles, then a hemi-octahedral impostor
  (12 by 12 frames, albedo with alpha and normal with depth, three-frame blend).
  Only LOD0 casts real shadows; mid distance needs a crown proxy or canopy darkening in the terrain,
  or forests look unanchored.
- **Alien flora** keeps the botanical logic and mutates one or two axes per species:
  topology, tropism targets, phyllotaxis angle, stem cross-section, appendage archetype,
  and a transmission colour different from reflectance.
  Per-planet gravity feeding droop and height makes a world's flora consistent, which is what sells it.
  The working rule is familiar structure with an unfamiliar surface, or the reverse, never both.

Nanite Foliage (voxels, assemblies, skinned wind) is where AAA is heading,
and Bevy's meshlets are not ready for alpha foliage.
What we can take from it is to store trees as a skeleton plus instanced twig modules in the offline format,
even though the runtime flattens them.

### Scatter

Our placement is already Horizon's model: rules evaluated per cell, deterministic, cell-local.
What we are missing is what Far Cry 5, Horizon and Snowdrop layer on top.

- **Clustered point processes.** Thomas or Matérn cluster parents hashed per coarse cell with a halo,
  multiplied by a per-species low-frequency patch field.
  This creates groves, thickets and gaps, and it is the biggest missing ingredient.
- **Size follows viability** (Far Cry 5), which gives forest edges their taper,
  plus an edge band where fit is between about 0.3 and 0.6 that favours shrubs and saplings.
- **Parent and child scatter** (Snowdrop): a boulder spawns moss, ferns and pebbles in a ring,
  a tree spawns deadfall and saplings.
  It is deterministic when the child's hash comes from the parent's id.
- **Terrain attributes beyond slope**: curvature, multi-radius topographic position,
  aspect, and a drainage signal for the wetness index.
- **Rocks placed by geology**: outcrops at convex slope breaks, scree fans below cliffs,
  boulders in drainage lines, and cliff meshes on slopes past about 45 degrees.
- **Ranked blue-noise tiles** (Yuksel sample elimination) are an alternative to our jittered grids
  that also gives density LOD for free, since far cells draw only a prefix.

Anything with a collider stays on the CPU in `strata-flora`.
Pure visuals such as grass, pebbles and twigs can move to GPU compute later, fed the same cell seed.

### Terrain look

- **Erosion.** The runevision erosion filter (March 2026) evaluates each point in isolation,
  so it works per cell and fits our pure-function-of-the-seed rule once ported to `libm`.
  It also outputs a ridge and drainage map that materials and vegetation can read.
- **Hex tiling** (Mikkelsen 2022) and **height-based layer blending** replace linear blend mush.
- **Biplanar mapping on steep slopes** fixes stretched cliffs.
- **A seed-derived macro colour map** that terrain fades to at distance,
  and that grass roots and tree tints read, ties everything into one landscape at range.
- **Object bottoms blend into the ground** by sampling the same colour map and splat by world XZ.

### Tooling

- **Content-addressed builds.** Key every node by `blake3(processor, VERSION, params, seed, inputs)`,
  cache under `target/`, and project a manifest into `assets/`.
  A per-processor `VERSION` constant is how we invalidate a cache when the algorithm changes.
  Bevy's `AssetProcessor` is the wrong driver, because its model is one source file in, one file out.
- **Crates** that fit Bevy's pins and our rules:
  `meshopt` 0.6.2 (the version Bevy 0.19.1 already pins), `obvhs` for CPU ray-traced AO, thickness and impostor bakes,
  `fast-surface-nets` for SDF rocks, `etagere` for atlases, `bevy_mikktspace` for tangents,
  `ctt` for BC7 and BC5, `insta` for stats snapshots, `image-compare` and `nv-flip` for golden images.
- **Avoid** SpeedTree and Houdini in the build (licence servers, not code),
  bpy in the build (version drift), AI models inside the build (not reproducible),
  `dssim-core` (AGPL), and any second copy of `wgpu` or `meshopt` outside Bevy's pins.
- **AI generation** (TRELLIS.2 is MIT) fits only as a pinned, hashed, licence-annotated input,
  the same way a Poly Haven scan does: a leaf exemplar or a rock silhouette target.
  `docs/SETTING.md` already names local TRELLIS generation, so this is a decision to confirm, not a new idea.
- **Review.** Contact sheets per family, fixed turntable rigs, black-silhouette sheets at 64 px
  to catch too little large-scale variation, and triangle, texel density and alpha-coverage budgets in CI.
  Curated seeds, rather than every seed, are how shipped procedural sets hold their quality.

## 3. The proposed shape

The rule is the one `veyra-buildings` already follows.
ashlar is the library: the content-step split, the material engine, the baked-file loader.
The game owns its generators and its recipes, and it runs its own content step.

The test for where a piece of work goes is whether another ashlar game would want it.
If it would, it goes into ashlar directly: ashlar is ours, so a gap is a change we make there,
not a workaround we build in the game.
If it encodes our art direction or our algorithm choices, it stays in the game.
Changes to ashlar are visible here with no push, since the dependency is a path,
and they have to pass ashlar's own `just ci`.

### Who owns what

| Concern | Owner | Why there |
| --- | --- | --- |
| Tree and plant generator, species recipes | New game crate `veyra-flora`, no Bevy | Our art direction and our algorithm |
| Rock and cliff recipes, geology rules | New game crate `veyra-rocks`, no Bevy | Our art direction; the SDF and sweep shapes they use are ashlar's |
| Bark, leaf, rock and terrain-layer graphs | The game crates, as `MaterialGraph`s; ashlar stdlib where it fits | Graphs are content; ashlar's are examples we may reuse |
| Buildings | `veyra-buildings`, plus `ashlar-showcase` kits where they fit | Unchanged; the showcase's sci-fi habitat and furniture are free content |
| The content step | A new `tools/content` binary over `ashlar-content`, run by `just content` | The kernel and the graph engine stay out of `game` and `server` |
| Placement and scatter rules | `strata-flora` | They depend on worldgen, and the server needs them |
| Baked asset format and its reader | ashlar's new asset kind, read with plain `ashlar` | The server reads colliders without linking a generator or Bevy |
| Grass on open terrain | `strata-foliage` storage buffers | ashlar strands are two orders too heavy for this |
| Moss and grass on rocks, ruins, buildings | ashlar strands | This is exactly what strands were built for |

The format is split from the generators on purpose.
The server needs to read "species 7, variant 3 has a trunk of this radius",
and it should not compile a growth simulation to do it.
`just server-check` should assert that `veyra-flora` and `veyra-rocks` stay out of the server graph,
the same way it guards `ashlar-manifold` today.

### The one real architectural change: the world depends on a build artifact

ADR 0005 has the tree shape derived from the seed at runtime.
Moving generation offline means the world is the seed plus a versioned asset library.

That sounds like it breaks determinism, but it tightens it.
The library ships as bytes, so every client and the server read identical data,
and the generators no longer need correctly rounded floats to agree across platforms.
Only placement stays under the worldgen float rule.
This needs an ADR 0005 amendment and a library version in the handshake,
so a client with a different library is refused like a client with a different protocol.

Generated files are regenerated output and are never committed.
`assets/` will need a split between committed data (RON, shaders, UI) and a generated subtree that is gitignored.

### What goes into ashlar

These are gaps any ashlar game would hit, so we close them in ashlar:

1. **Alpha and transmission in materials.** An opacity output on `PbrOutput`, a thickness map,
   an alpha mode on `MaterialDefinition`, and the matching arms in `create_material`.
   `ashlar-bevy/src/cards.rs` already uses `AlphaMode::Mask`, so there is precedent.
   ADR 0003 put transparency out of scope; this reopens it.
2. **Non-square bakes and atlases.** A graph can write a leaf or twig sheet,
   and a packer (`etagere`) lays out sheets and impostor frames.
3. **Extra vertex data.** Vertex colours and extra attributes on `TriangleMesh` and in the baked file.
   Wind hierarchy, AO, height and curvature for the stylised shaders all need somewhere to live.
4. **A baked asset kind beside buildings.** A mesh set with levels, colliders and attributes
   that `AshlarPlugin` loads and spawns with `VisibilityRange`s, so trees and rocks are not dressed up as buildings.
5. **Organic shapes in `Geometry`.** Tube sweeps along a curve, SDF level sets,
   smoothing and warps through the Manifold kernel's `from_sdf`, `warp`, `smooth_out` and `refine`,
   with displacement described as data.
   Rocks, cliffs and tree junctions all build on these.
   `LodPolicy` also has to learn to simplify hulls and level sets, which it never does today.
6. **An impostor baker.** Hemi-octahedral frames baked in the content step,
   CPU ray-cast with `obvhs` so CI gets identical bytes, plus the runtime material in `ashlar-bevy`.
7. **World-space masks in a shipped game.** Moss and snow on top of rocks without the `shader` tool feature,
   as a precompiled material extension rather than a runtime-generated one.
8. **Content-addressed caching in `ashlar-content`**, keyed by processor, version, parameters and seed,
   before bakes get slow enough to hurt iteration.

What stays in the game: the growth algorithm and species recipes in `veyra-flora`,
rock and cliff recipes in `veyra-rocks`, our bark, leaf and terrain graphs,
and `tools/content`, which calls both.
Spike S2 decides whether items 3 and 4 are one change or two.

### Built for a stylised look

Painterly shading is later work, but it constrains the assets now.
This may seem backwards, since stylised usually means less detail.
In practice a stylised look moves the detail from textures into shapes and into data the shader reads,
and that data has to be baked in by the generators:

- **Shape over texture.** Crowns read as a few large, clumped masses with clear gaps.
  Leaf cards are bigger, painted-looking clusters, not photographic leaves.
  Bark and rock carry bold, low-frequency forms; fine noise fights a painterly shader.
- **Soft normals.** Foliage normals come mostly from the crown envelope, 80% or more,
  so a crown shades as one soft volume.
  The same trick on rocks, with normals smoothed from a proxy, gives the soft stylised read.
- **Baked shading data.** Every generated mesh carries vertex AO, a normalised height in the asset,
  and a cavity or curvature term.
  Those drive the gradient ramps, rim light and shadow tinting that painterly lighting relies on.
- **Controlled frequency in materials.** Graph bakes let us choose the detail scale per material,
  and quantised gradients and curvature edge highlights are graph nodes we already have.
  This is one of the reasons to drop the photo sets: they clash with a stylised look at every distance.
- **One colour story.** The macro colour map, terrain tints, grass roots and tree tints come from one source,
  so the palette can be art-directed per biome and later per time of day.

### Budgets

Trees follow the decided 15k cap at the nearest level:

| Level | Triangles | Range (to tune) | Shadows | Wind |
| --- | --- | --- | --- | --- |
| LOD0 | up to 15k, branches plus cluster cards | 0 to about 60 m | Casts | Full pivot hierarchy |
| LOD1 | about 5k | to about 150 m | Crown proxy | Trunk and first branches |
| LOD2 | about 1.5k | to about 350 m | None, or far-cascade proxy | Trunk sway |
| Impostor | 2 per tree | beyond | Canopy darkening in the terrain | Global sway |

15k is almost six times today's 2600.
The foliage spike measured that only near LOD shadows are affordable (2 ms against 17 ms for all levels),
and that number was taken with the small trees.
Spike S2 has to repeat it at 15k before we commit to the near range.

### What the game needs beyond the pipeline

These do not wait for the pipeline, and several change what every seed looks like:

- Clustered scatter, size by fit, edge bands, parent and child scatter in `strata-flora`.
- Curvature, topographic position, aspect and drainage in `strata-worldgen`.
- The erosion filter port, which changes the terrain of every seed.
- Hex tiling, height blending, biplanar on steep slopes and the macro colour map in the terrain shader.
- Grass clumping, terrain-coloured roots and rounded normals, following Ghost of Tsushima.
- Rock placement with colliders, and a server-side collider for each rock placement.
- Replacing the runtime `ashlar-manifold` link in `apps/game` with the content step and `AshlarPlugin`,
  as the ashlar skills prescribe.

## 4. Spikes to de-risk it

Each spike answers one question, like the foliage spike did.
They are disposable: we mine them for the plan and do not merge them.

| Spike | Question | Done when |
| --- | --- | --- |
| S0 | Fix the build against ashlar HEAD | `just ci` passes on the migration |
| S1 Tree | Do space colonisation, pipe radii, gravity bend and clumped cluster cards with crown normals give a stylised tree that clearly beats `build_tree`? | Turntables and a 64 px silhouette sheet of 16 seeds for one broadleaf and one conifer, LOD0 under 15k, next to today's trees |
| S2 Container | What shape does ashlar's mesh-set asset kind take (attributes, levels, colliders), and can the runtime carry 15k trees? | 20 species by 8 variants by 4 levels load; upload time, memory and near-shadow cost at 15k measured on the woodland fixture |
| S3 Impostor | Does a hemi-octahedral impostor hide the switch under MSAA and TAA in Bevy 0.19? | No visible pop in a still at the switch distance; Bevy issue #17643 checked |
| S4 Rock | Manifold `from_sdf` or `fast-surface-nets` for rocks? | 8 rocks of one geology with LODs and hulls, kernel canary green, timings for both |
| S5 Terrain material | Can stylised graph-baked layers with hex tiling and height blending replace the Poly Haven set, and bark with it? | The four look-study fixtures, before and after, at matched cost, with no photo texture left |
| S6 Scatter | Do clusters, size by fit and parent and child rules make forests read as natural? | Worldmap cover plus fixture screenshots; the pair-correlation of placements compared to the old placement |
| S7 Erosion | Does the erosion filter port pay for changing every seed? | Worldmap relief and mountain fixtures; per-sample cost against today's 852 ns |

S1, S4 and S6 are the ones that most change what the game looks like.
S2 and S3 decide whether the runtime can carry it.

## 5. Open questions

The first round is answered above. These remain, and none of them blocks S0 or S1.

1. **Minimum GPU.** Every number we have is from an RTX 4090.
   The 15k cap and the LOD0 range should be tuned against the weakest card we intend to support.
2. **Phase 5e.** Its octahedral baker becomes spike S3 and then the pipeline's impostor baker.
   Its alien species and biome wait for the alien-share decision.
   Should the plan formally fold 5e into this work?
3. **The generated asset tree.** Gitignore a subtree of `assets/`, or write generated files to a separate root
   that the game adds as a second asset source?
   The second keeps committed and generated files from ever mixing.

## 6. Out of scope for this discovery

GPU compute placement, virtual texturing, Nanite-style virtual geometry, destructible trees,
trampling and interaction, and seasons.
Each was looked at and is either not ready in Bevy 0.19 or not needed before the basics above.

## 7. Key references

Trees and plants:

- Runions et al. 2007, space colonisation: https://algorithmicbotany.org/papers/colonization.egwnp2007.large.pdf
- Palubicki et al. 2009, self-organising trees: https://algorithmicbotany.org/papers/selforg.sig2009.html
- Unreal Procedural Vegetation Editor: https://dev.epicgames.com/documentation/unreal-engine/procedural-vegetation-editor-in-unreal-engine
- GPU Gems 3 ch. 16, Crysis vegetation: https://developer.nvidia.com/gpugems/gpugems3/part-iii-rendering/chapter-16-vegetation-procedural-animation-and-shading-crysis
- Pivot Painter 2: https://dev.epicgames.com/documentation/unreal-engine/pivot-painter-tool-2.0-in-unreal-engine
- Hemi-octahedral impostors: https://www.yummers.dev/hemi-octahedral-impostors.html
- Runions et al. 2005, leaf venation: https://algorithmicbotany.org/papers/venation.sig2005.html
- `bevy_procedural_tree` 0.4 (Bevy 0.19): https://github.com/Affinator/bevy_procedural_tree
- ez-tree: https://github.com/dgreenheck/ez-tree

Scatter and terrain:

- Horizon Zero Dawn GPU placement: https://www.guerrilla-games.com/read/gpu-based-procedural-placement-in-horizon-zero-dawn
- Far Cry 5 procedural world: https://www.gdcvault.com/play/1025557/
- Snowdrop scatter for Pandora: https://www.massive.se/blog/games-technology/snowdrop/crafting-pandoras-breathtaking-landscape-with-snowdrop/
- EcoBrush: https://hal.science/hal-01519852
- Yuksel sample elimination: http://www.cemyuksel.com/research/sampleelimination/
- runevision erosion filter: https://blog.runevision.com/2026/03/fast-and-gorgeous-erosion-filter.html
- Mikkelsen hex tiling: https://jcgt.org/published/0011/03/05/
- Ghost of Tsushima grass: https://gdcvault.com/play/1027033/

Tooling:

- Houdini PDG: https://www.sidefx.com/docs/houdini/tops/intro.html
- Unreal derived data cache: https://dev.epicgames.com/documentation/en-us/unreal-engine/using-derived-data-cache-in-unreal-engine
- TRELLIS.2: https://github.com/microsoft/TRELLIS.2
