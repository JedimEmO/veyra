# Foliage and vegetation: reference mood board

Status: exploratory, 2026-09-12. Not a plan. A curated set of techniques
and sources for procedural placement, cheap instanced rendering, wind
animation, foliage shading and procedural plant generation, collected
before deciding anything. Each entry says what the idea is and why it is
on the board. Sources a research pass could not open directly are marked
"unverified"; treat those as leads.

Where a claim is about Bevy it was checked against the 0.19.1 sources in
`~/.cargo/registry`, not docs from memory.

## What we already have to hang this on

- Regions: `RegionCoord { x, z, lod }`, 64 m base cells, quadtree
  selection in `strata-stream` as a pure function of observer positions
  (ADR 0003). Foliage placement per 64 m cell with a per-cell hash of the
  world seed is the natural unit. Far Cry 5 and Flying Wild Hog's Biome
  Painter both landed on 64 m tiles independently.
- `worldgen::Sample` gives height, the two heaviest biomes with weights,
  and a surface material index at any point, allocation free and the same
  bits on every OS. That is the density function placement wants, and the
  server can evaluate it too, so trees can be colliders and gameplay
  objects with identical positions on both ends.
- The terrain mesh already carries a biome splat and a per-vertex tint
  attribute, so "grass tinted by the ground under it" is a lookup we
  already compute.
- Bevy 0.19.1 built-ins that matter (see section 6): GPU preprocessing
  with indirect draws and GPU frustum culling on by default,
  `VisibilityRange` with dithered crossfade between LOD entities,
  `OcclusionCulling` (two-phase Hi-Z, needs a depth prepass), `MeshTag`
  for a per-instance u32 readable in the shader, `AlphaMode::Mask` and
  `AlphaMode::AlphaToCoverage`, and an experimental meshlet renderer.

## 1. Placement and distribution

The recurring shape across every shipped system: a density function
evaluated per cell, a deterministic low-discrepancy point pattern
thresholded against it, big things placed first with exclusion radii,
grass filled in last, and only the nearest rings get the small stuff.

### Shipped systems

- **GPU-Based Run-Time Procedural Placement in Horizon Zero Dawn**, Jaap
  van Muijden, GDC 2017. https://gdcvault.com/play/1024700/GPU-Based-Run-Time-Procedural
  and https://www.guerrilla-games.com/read/gpu-based-procedural-placement-in-horizon-zero-dawn
  Artists paint density maps and a rule graph ("grass on dirt, moss on
  rock under a canopy layer"); a compute pipeline evaluates it per tile
  around the player using dithered footprint textures, one texel per
  candidate slot, jittered by noise. Nothing is baked; placement is a
  function of the terrain, which is exactly our situation.
- **Between Tech and Art: The Vegetation of Horizon Zero Dawn**, Gilbert
  Sanders, GDC 2018. https://www.gdcvault.com/play/1025530/Between-Tech-and-Art-The
  Companion talk on the meshes, shaders and budgets that consume the
  placed instances. Behind Vault registration; no numbers in the abstract.
- **Procedural World Generation of Far Cry 5**, Etienne Carrier, GDC 2018.
  https://www.gdcvault.com/play/1025557/Procedural-World-Generation-of-Far
  Written notes: https://christianjmills.com/posts/procedural-tools-far-cry-5-notes/index.html
  Species compete by a viability score built from slope, altitude,
  occlusion, illumination and distance to water; highest viability wins a
  spot; a density ramp by age and size. Offline in Houdini, but the
  viability-as-competition idea is cheap enough to do at runtime per cell.
  Minimum generation sector was 64 x 64 m.
- **Biome Painter: Populating Massive Worlds**, Krzysztof Narkowicz, 2019.
  https://knarkowicz.wordpress.com/2019/08/11/biome-painter-populating-massive-worlds/
  The most directly reusable write-up on the board. Precomputed spawn
  points and precomputed random numbers per 64 m tile; a dither weight per
  point so raising density adds points without moving existing ones; large
  objects first with collision tests, then bushes, then grass; LOD rings
  (everything near, larger objects further, only the largest and impostors
  at the horizon); placement stored as colour maps at about 2 bytes per m².
- **Procedural Grass in Ghost of Tsushima**, Eric Wohllaib, GDC 2021.
  https://gdcvault.com/play/1027033/Advanced-Graphics-Summit-Procedural-Grass
  (one pass reported the id 1027214 for the same talk), video
  https://www.youtube.com/watch?v=Ibe1JBF5i5Y. Blades generated on the GPU
  from terrain textures, no CPU instance list at all; Voronoi clumps give
  each blade a clump centre that drives height, facing, lean and colour.
  Referenced again in sections 2, 3 and 4.
- **Building and Loading Ghost of Tsushima**, Sucker Punch, GDC 2021.
  https://media.gdcvault.com/GDC+2021/ghost_streaming_gdc2021.pdf
  (unverified, large PDF). Streaming side of the same world.
- **The Natural World of Assassin's Creed Valhalla**, GDC 2021.
  https://www.gdcvault.com/play/1027284/Art-Direction-Summit-The-Natural
  Art direction of biomes and forests built with procedural tooling and
  SpeedTree; useful for the "what does a readable forest need" question.
- **Avatar: Frontiers of Pandora, Snowdrop**, Massive, GDC 2024 and blog.
  https://www.massive.se/blog/games-technology/snowdrop/crafting-pandoras-breathtaking-landscape-with-snowdrop/
  Node-based scattering with parent-child placement rules; about 70 M
  instances over 32 km². Also the strongest alien-flora reference (section 5).

### Point patterns

- **Fast Poisson Disk Sampling in Arbitrary Dimensions**, Bridson, 2007.
  https://www.cs.ubc.ca/~rbridson/docs/bridson-siggraph07-poissondisk.pdf
  The standard O(n) blue-noise scatter: background grid of cell size
  r / sqrt(d), k = 30 candidates in the annulus [r, 2r]. Needs an active
  list, so it is an offline or per-tile job, not a per-sample function.
- **Blue Noise Point Sets as Progressive Tiling**, Sean Barrett.
  https://nothings.org/gamedev/blue_noise/
  A progressive blue-noise set stored as a small image; thresholding the
  image gives a valid subset at any density, so trees, bushes and grass can
  come from one tile at different thresholds and density can be edited
  locally without disturbing neighbours. Deterministic, allocation free,
  tileable: the best match for a per-cell hash scheme.
- **Wang Tiles for Image and Texture Generation**, Cohen et al., 2003.
  https://www.cs.jhu.edu/~misha/Spring25/Readings/Cohen03.pdf
  and **Recursive Wang Tiles for Real-Time Blue Noise**, Kopf et al., 2006.
  https://dl.acm.org/doi/10.1145/1141911.1141916
  Precomputed Poisson point sets on tiles that stay blue-noise across tile
  edges; the recursive version refines only where more density is wanted,
  which maps onto a quadtree of regions.
- **Hashing** (Procedural GPU Patterns), Catlike Coding.
  https://catlikecoding.com/godot/procedural-gpu-patterns/04-hashing/
  A worked xxHash-style integer hash from cell coordinates to a float; the
  "per-cell scatter with no state" primitive. `strata-core` already has an
  `rng` module; the same discipline (integer hash of seed, cell, slot
  index) keeps client and server bit-identical.
- **Stochastic Sampling in Computer Graphics**, Cook, 1986.
  https://dl.acm.org/doi/10.1145/7529.8927
  Why jitter beats a grid: aliasing becomes noise, which the eye forgives.

### Ecosystem realism

- **Realistic Modeling and Rendering of Plant Ecosystems**, Deussen et al.,
  SIGGRAPH 1998. https://algorithmicbotany.org/papers/ecosys.sig98.pdf
  Simulated competition for light and space among coarse surrogates, then
  instantiate detailed models only for survivors. Too slow at runtime, but
  it defines what "looks like a real distribution" means: self-thinning,
  size-dependent spacing, species clustering.
- **Generating Spatial Distributions for Multilevel Models of Plant
  Communities**, Lane and Prusinkiewicz, 2002.
  https://algorithmicbotany.org/papers/eco.gi2002.pdf
  Names the two routes: local-to-global (simulate individuals) and
  global-to-local (sample positions from a target density map). Games do
  the second; the paper shows how far it can be pushed.
- **Polygonal Map Generation for Games**, Amit Patel.
  https://www-cs-students.stanford.edu/~amitp/game-programming/polygon-map-generation/
  The elevation x moisture lookup table for biomes. Our biome registry is
  already a richer version of this; vegetation species are the next column
  in that table.

### Runtime shape

- Chunk-local deterministic RNG from one world seed is the Minecraft
  pattern (https://www.alanzucconi.com/2022/06/05/minecraft-world-generation/).
  Every source above that runs at runtime derives randomness from a hash of
  tile and slot, never from a stream.
- **GPU Run-Time Procedural Placement on Terrain**, Kacper Szwajka
  (unverified). https://medium.com/@kacper.szwajka842/gpu-run-time-procedural-placement-on-terrain-cc874e39bbfb
  An independent Horizon-style implementation: instance buffers only for
  chunks near the camera, randomness from a chunk and cell hash.

## 2. Rendering many instances cheaply

### GPU-driven pipelines

- **GPU-Driven Rendering Pipelines**, Haar and Aaltonen, SIGGRAPH 2015.
  https://www.advances.realtimerendering.com/s2015/aaltonenhaar_siggraph2015_combined_final_footer_220dpi.pdf
  The Assassin's Creed Unity talk: visibility decided on the GPU, indirect
  draws from compacted lists, no CPU round trip. Bevy 0.19 does the
  mesh-level version of this by default.
- **Experiments in GPU-Based Occlusion Culling**, Kostas Anagnostou, 2017.
  https://interplayoflight.wordpress.com/2017/11/15/experiments-in-gpu-based-occlusion-culling/
  Hi-Z chain, AABB tests in compute, stream compaction, indirect draw.
  Frame time 5+ ms to 1.5 ms with a 0.1 ms cull pass on a GTX 970. This is
  what Bevy's `OcclusionCulling` component does internally.
- **Masked Software Occlusion Culling**, Hasselgren et al., Intel 2016.
  https://fileadmin.cs.lth.se/graphics/research/papers/2016/culling/culling.pdf
  CPU-side alternative when whole batches can be rejected before submit.
- **Half Baked: Dynamic Occlusion Culling**, Angelo Pesce, 2023.
  http://c0de517e.blogspot.com/2023/03/half-baked-dynamic-occlusion-culling.html
  Occlusion in fully dynamic scenes; relevant if trees themselves are the
  main occluders in a forest.

### Grass

- **Responsive Real-Time Grass Rendering for General 3D Scenes**, Jahrmann
  and Wimmer, i3D 2017. https://www.cg.tuwien.ac.at/research/publications/2017/JAHRMANN-2017-RRTG/
  Each blade a tessellated quadratic Bezier with a per-blade physical model
  (gravity, wind, collision) and culling by distance, frustum and
  orientation. Reference code: https://github.com/moneimne/VulkanGrassRendering.
  wgpu has no tessellation; the blade would be generated in compute or
  expanded from an instance in the vertex shader instead.
- **Ghost of Tsushima grass** (talk above). Implementation-level restatement
  at https://tigerabrodi.blog/grass-in-ghost-of-tsushima (secondary) and an
  open Unity reproduction at https://github.com/cainrademan/Unity-Grass:
  one compute thread per blade appends to a buffer after jitter, frustum
  and distance culling, Voronoi clumping; the vertex shader evaluates a
  cubic Bezier for 15 vertices near, 7 mid; far grass is a terrain texture
  whose colour is blended toward the grass tip colour.
- **Rendering Countless Blades of Waving Grass**, Pelzer, GPU Gems 1 ch. 7.
  https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-7-rendering-countless-blades-waving-grass
  The 2004 answer: three crossed alpha quads per tuft. Still the right
  answer for the mid ring where blades are a pixel wide.
- **Toward Photorealism in Virtual Botany**, Whatley, GPU Gems 2 ch. 1.
  https://developer.nvidia.com/gpugems/GPUGems2/gpugems2_chapter01.html
  LOD swap, batching and clumped distributions for large plant fields.
- **Procedural grass rendering with mesh shaders**, AMD GPUOpen (unverified).
  https://gpuopen.com/learn/mesh_shaders/mesh_shaders-procedural_grass_rendering/
  Blade geometry from a task plus mesh shader; wgpu exposes mesh shaders
  only experimentally, so this is a "later" reference.
- **Modern Foliage Rendering**, Acerola (video). https://www.youtube.com/watch?v=jw00MbIJcrk
  Approachable survey of blade geometry versus cards, with the BotW
  comparison.
- Breath of the Wild grass, community analysis only (no first-party
  source): single-triangle blades, normal flip for two-sided shading,
  shadows off. https://www.resetera.com/threads/zelda-breath-of-the-wild-the-technical-analysis.8197/

### Trees: LOD chains and impostors

- **Octahedral Impostors**, Ryan Brucks, 2018. https://shaderbits.com/blog/octahedral-impostors
  Bake a lattice of views into an atlas, pick and blend the three nearest
  per pixel from the view vector, with depth for parallax and correct
  intersection. The technique behind Epic's Impostor Baker and Amplify
  Impostors (https://amplify.pt/unity/amplify-impostors/: instancing,
  dithered crossfade, depth write). The far-tree answer for this project
  unless meshlets turn out to be enough.
- **Extreme Model Simplification for Forest Rendering**, Fuhrmann, Umlauf
  and Mantler, 2005. https://doi.org/10.2312/NPH/NPH05/057-066
  Billboard clouds: a 110 k polygon tree as 11 oriented cards; forests of
  50 to 80 k trees interactive in 2005.
- **Next-Generation SpeedTree Rendering**, Kharlamov, Cantlay and
  Stepanenko, GPU Gems 3 ch. 4.
  https://developer.nvidia.com/gpugems/gpugems3/part-i-geometry/chapter-4-next-generation-speedtree-rendering
  Camera-facing leaf cards with depth offset for self-shadowing, and LOD
  crossfade by rendering both LODs with offset alpha-test thresholds under
  alpha-to-coverage.
- **SpeedTree LOD and billboard docs** (Cloudflare blocks fetches).
  https://docs.speedtree.com/doku.php?id=overview_level-ofdetail
  The canonical chain: full mesh, reduced mesh with leaf cards scaled by
  the LOD factor, multi-view billboard atlas shared by the forest.
- **Nanite Foliage**, Epic, UE 5.8 experimental.
  https://dev.epicgames.com/documentation/en-us/unreal-engine/nanite-foliage
  Assemblies (micro-instancing of branch parts, up to 65 k per assembly),
  voxel fallback at distance, skinned wind (100 k bones for 0.1 ms). The
  Witcher 4 demo tree: 41 M triangles, 12 parts, 2 160 instances, 850
  bones, 3.5 GB to 29 MB on disk. Out of reach as a technique, but the
  assembly idea (a tree is a few branch meshes instanced along a skeleton)
  is exactly what a procedural tree generator produces for free.

### Alpha

- **Hashed Alpha Testing**, Wyman and McGuire, i3D 2017.
  https://cwyman.org/papers/i3d17_hashedAlpha.pdf
  Stochastic threshold fixes alpha-tested foliage thinning to nothing at
  distance; composes with alpha-to-coverage and TAA.
- **To Z-Prepass or Not to Z-Prepass**, Angelo Pesce, 2020.
  https://interplayoflight.wordpress.com/2020/12/21/to-z-prepass-or-not-to-z-prepass/
  Alpha test in the prepass, equal depth test afterwards, so the texture
  fetch for the cutout is paid once. Our terrain already runs a prepass.
- **Anti-aliased Alpha Test: The Esoteric Alpha To Coverage**, Ben Golus
  (unverified, 403). https://bgolus.medium.com/anti-aliased-alpha-test-the-esoteric-alpha-to-coverage-8b177335ae4f
- **Dithered opacity for LOD crossfade**, Cesium blog, 2022.
  https://cesium.com/blog/2022/10/20/smoother-lod-transitions-in-cesium-for-unreal/
  Dither plus TAA as free translucency for the LOD switch. Bevy's
  `VisibilityRange` crossfade is this.
- **Temporal AA and the Quest for the Holy Trail**, Roberto Lopez Mendez.
  https://www.elopezr.com/temporal-aa-and-the-quest-for-the-holy-trail/
  Wind-bent foliage smears under TAA without motion vectors that include
  the vertex animation; anything we do in the vertex shader has to be
  reproducible for the previous frame too.
- Open gap: no primary source quantifies leaf-card overdraw in ms; only
  forum discussion and SpeedTree's leaf-collision docs on reducing overlap.

## 3. Wind

The whole field descends from one chapter and adds a wind field and a
hierarchy on top.

### The base model

- **Vegetation Procedural Animation and Shading in Crysis**, Tiago Sousa,
  GPU Gems 3 ch. 16.
  https://developer.nvidia.com/gpugems/gpugems3/part-iii-rendering/chapter-16-vegetation-procedural-animation-and-shading-crysis
  Main bending: the whole plant leans along the wind, scaled by height,
  and the displaced position is renormalised to its original length so
  bending is a rotation and nothing stretches. Detail bending: per-leaf
  flutter from wind strength only, driven by vertex colour: R edge
  amplitude, G per-leaf phase, B stiffness, A baked AO. Smoothed triangle
  waves at 1.975, 0.793, 0.375 and 0.193 Hz. Clean reimplementations at
  https://mtnphil.wordpress.com/2011/10/18/wind-animations-for-vegetation/
  and https://minifloppy.it/posts/2014/udk-wind-vertex-shader/.
- **GPU-Generated Procedural Wind Animations for Trees**, Zioma, GPU Gems 3
  ch. 6. https://developer.nvidia.com/gpugems/gpugems3/part-i-geometry/chapter-6-gpu-generated-procedural-wind-animations-trees
  Motion synthesised as a sum of frequency bands per branch rather than a
  solved oscillator; two blended rigid transforms per vertex give flexible
  bending from a rigid hierarchy.

### Hierarchy without a skeleton

- **Pivot Painter 2.0**, Epic.
  https://dev.epicgames.com/documentation/en-us/unreal-engine/pivot-painter-tool-2.0-in-unreal-engine
  Per-branch pivot, extent, basis, parent index, depth and a random value
  baked into textures so the vertex shader rotates each branch about its
  own pivot and inherits its parent's motion; four levels, 30 k elements.
  A procedural tree generator knows all of this at generation time and can
  write it straight into vertex attributes instead of textures.
- **Book of the Dead: wind**, Unity, 2018.
  https://unity.com/blog/technology/book-of-the-dead-quixel-wind-scene-building-and-content-optimization-tricks
  Three tiers by asset type: hierarchical pivots for trees (trunk 0,
  branch level A in 0..1, level B above 1, encoded in one vertex channel),
  single pivot for grass and bushes, pure procedural for the rest.
- **SpeedTree wind** (fetch blocked). https://docs.speedtree.com/doku.php?id=advancewind
  Global motion, then branch motion per level, then leaf ripple (cheap
  normal-axis oscillation) and tumble (full pivot rotation, high tier
  only); response curves shaped per wind strength; a "wind LOD" switch
  keeps global motion alive on billboards so far trees stay in phase.
- **Alan Wake 2 plant bones**, Remedy.
  https://www.remedygames.com/article/how-northlight-makes-alan-wake-2-shine
  The expensive end: real joint rigs per plant (about 300 k active bones
  in a scene) driven by SDF "wind box" volumes. On the board as the
  ceiling, and for the wind-box idea of carving wind-free volumes.

### Wind fields

- **Blowing from the West: Simulating Wind in Ghost of Tsushima**, Bill
  Rockenbeck, GDC 2021. https://gdcvault.com/play/1027124/Blowing-from-the-West-Simulating
  slides https://media.gdcvault.com/GDC+2021/Rockenbeck-Bill-Blowing+from+the+West.pdf
  One global wind vector whose magnitude is modulated by time-varying
  Perlin noise for travelling gusts, plus "vorticles" (position, radius,
  direction) summed brute force for local vortices and sword strikes; one
  field shared by grass, cloth and particles. The grass talk adds a
  displacement buffer rendered from character positions for trampling.
- **Interactive Wind and Vegetation in God of War**, Sean Feeley, GDC 2019.
  https://gdcvault.com/play/1026036/Interactive-Wind-and-Vegetation-in
  A "wind motor" produces a spatially varying 3D field (sparse flow plus
  scale-stable fractal noise) sampled by "wind receivers"; boneless sway;
  billboard-cloud LODs.
- Horizon Zero Dawn's wind is an omnipresent force-field box around the
  player with per-object response weights (Sanders talk above; no
  Forbidden West wind talk exists).
- **Woodbound wind lines**, 80.lv, 2017.
  https://80.lv/articles/woodbound-making-a-living-world-with-wind-in-ue4
  A world-aligned panning noise texture also lerps grass colour toward a
  brighter sheen, so gusts are visible as light moving across the field,
  not just geometry.
- **Grass LOD tricks in Godot**, hexaquo.at.
  https://hexaquo.at/pages/grass-rendering-series-part-4-level-of-detail-tricks-for-infinite-plains-of-grass-in-godot/
  Use the identical wind formula on the full mesh and on the impostor so
  the LOD switch does not change phase; fade only the high-frequency terms
  with distance.

### Physically motivated (for realism, not necessarily for runtime)

- **Physically Guided Animation of Trees**, Habel, Kusternig and Wimmer,
  Eurographics 2009. https://www.cg.tuwien.ac.at/research/publications/2009/Habel_09_PGT/Habel_09_PGT-draft.pdf
  Per-branch nonlinear deformation plus a statistical wind response
  precomputed in frequency space and sampled from a 2D motion texture;
  thousands of branches in one pass. The best "looks right" model that is
  still a vertex shader.
- **1/f^beta noise-based real-time animation of trees**, Ota et al., 2003.
  https://ieeexplore.ieee.org/document/1214447/
  Real foliage motion has a pink-noise spectrum; a sum of a few sines does
  not. A cheap upgrade over Crysis's four fixed frequencies.
- **Wind Projection Basis for Real-Time Animation of Trees**, Diener,
  Rodriguez, Baboud and Reveret, 2009. https://maverick.inria.fr/Publications/2009/DRBR09/
  Precomputed modal basis from a mechanical tree model; arbitrary wind
  projects onto it on the GPU.
- **Fast Simulation of Realistic Trees**, Weber, 2008.
  https://ieeexplore.ieee.org/document/4497899/

### Shader details worth keeping

- Root pinned by scaling displacement with distance from the pivot; bend
  as rotation with renormalisation (Sousa); per-instance phase from a
  position hash so a field never moves in unison (Roystan,
  https://roystan.net/articles/grass-shader/); normals rebuilt after
  bending by evaluating the displacement at tangent-offset probes and
  crossing the differences
  (https://unity-trouble-atlas.7colorsgame.com/en/article/unity-shader-graph-vertex-displacement-normal-reconstruction/);
  Unreal's `instance_world_position_offset_disable_distance` as the model
  for switching wind off entirely beyond a range.

## 4. Shading foliage

- **Crysis shading** (Sousa, above): double-sided alpha-tested leaves,
  Phong plus a back-lighting term from an artist thickness map, wrap term
  `saturate(dot(-N, L) * 0.6 + 0.4)`, per-vertex AO baked into a channel.
- **Two Sided Foliage shading model**, Epic.
  https://dev.epicgames.com/documentation/unreal-engine/shading-models-in-unreal-engine
  A dedicated thin-transmission model because general subsurface is wrong
  for leaves; the subsurface colour input doubles as a vein and stem mask.
  Bevy's `StandardMaterial` has diffuse transmission and a
  `TransmittedShadowReceiver`; whether that is enough or a custom material
  is needed is an open question.
- **Physically Based Real-Time Translucency for Leaves**, Habel, Kusternig
  and Wimmer, EGSR 2007. https://www.cg.tuwien.ac.at/research/publications/2007/Habel_2007_RTT/
  Measured leaf translucency as a compact directional-basis convolution.
- **Unity HDRP subsurface docs**: the distinction between "translucent"
  (transmission only, right for leaves) and full subsurface (screen-space
  blur). https://docs.unity3d.com/Packages/com.unity.render-pipelines.high-definition@14.0/manual/Subsurface-Scattering.html
- **Normals**: spherical or ellipsoid normals copied onto leaf cards so a
  canopy shades as one volume (https://forums.unrealengine.com/t/spherical-normals-for-trees-blender/98732,
  https://polycount.com/discussion/125920/correct-vertex-normals-for-foliage,
  SpeedTree's "parent puffiness"). Grass takes the terrain normal at its
  root so blade and ground are one surface (Daniel Ilett's BotW grass,
  https://danielilett.com/2021-08-24-tut5-17-stylised-grass/, code
  https://github.com/daniel-ilett/shaders-botw-grass).
- **Colour**: Tsushima's clump colour and far-terrain blend toward the
  grass tip colour; Unity Terrain's healthy/dry colour pair per prototype
  (https://docs.unity3d.com/6000.1/Documentation/Manual/terrain-Grass.html);
  our existing per-vertex terrain tint is the natural source for the root
  colour, and `MeshTag` or a hash of the instance position for the
  per-instance variation.
- **Shadows**: grass casts contact shadows only, no dynamic shadow, and
  wind is disabled past a distance (Unreal GrassVariety,
  https://dev.epicgames.com/documentation/en-us/unreal-engine/python-api/class/GrassVariety?application_version=5.3;
  contact shadows in https://dev.epicgames.com/documentation/unreal-engine/shadowing-in-unreal-engine).
- **Bevy alpha**: `AlphaMode::Mask(t)` and `AlphaMode::AlphaToCoverage`
  both render in the opaque phase; alpha-to-coverage degrades to a 0.5
  mask without MSAA (verified in `bevy_pbr` 0.19.1 `material.rs`). The
  historical issue of masked materials casting uncut shadows is closed
  (https://github.com/bevyengine/bevy/issues/4969); worth a quick visual
  check with our own material.

## 5. Procedural plants, including alien ones

### Foundations

- **Creation and Rendering of Realistic Trees**, Weber and Penn, SIGGRAPH
  1995. https://history.siggraph.org/learning/creation-and-rendering-of-realistic-trees-by-weber-and-penn/
  Nested generalised cylinders with per-level curvature, taper, split
  count and angle, a pruning envelope, and LOD rules. Every practical tree
  tool below is this paper with a UI. About 30 parameters per species,
  which is a RON file in our registry.
- **The Algorithmic Beauty of Plants**, Prusinkiewicz and Lindenmayer,
  1990, free PDF. https://algorithmicbotany.org/papers/abop/abop.pdf
  L-systems, bracketed, stochastic, parametric. The natural language for
  alien morphologies because rewriting rules have no bias toward Earth
  trees.
- **Modeling Trees with a Space Colonization Algorithm**, Runions, Lane and
  Prusinkiewicz, 2007. https://algorithmicbotany.org/papers/colonization.egwnp2007.pdf
  An attraction-point cloud shapes the crown; two distances, `D_attr` and
  `D_kill`, control everything. Crown shape in, plausible branching out;
  give it a non-botanical point cloud and it grows a non-botanical plant.
- **Self-Organizing Tree Models for Image Synthesis**, Palubicki et al.,
  SIGGRAPH 2009. https://algorithmicbotany.org/papers/selforg.sig2009.html
  Buds compete for light through a shadow field with apical control;
  produces the most convincing silhouettes of the family, and also the
  forest-scale interaction (trees shaped by their neighbours) that
  section 1's ecosystem papers want.
- **TreeSketch**, Longay et al., 2012.
  https://algorithmicbotany.org/papers/TreeSketch.SBM2012.html

### Implementations to read

- **proctree.js**, https://github.com/supereggbert/proctree.js: a small,
  readable generator (levels, segments, branch factor, taper, kink, twist,
  sweep, clump, seed) that emits a mesh plus twig cards; the right size to
  port.
- **Arbaro**, https://github.com/wdiestel/arbaro: faithful Weber and Penn
  in Java with XML species presets to steal parameters from.
- **Sapling Tree Gen**, https://extensions.blender.org/add-ons/sapling-tree-gen/
  (403 on fetch) and **TreeIt**, https://www.evolved-software.com/treeit/treeit:
  offline tools for authoring reference trees and baking impostors.
- **SideFX Labs tree tools**, https://www.sidefx.com/products/houdini/world-building/foliage/
  (unverified): hybrid IFS and space colonisation from a crown envelope.
- Rust: **dcc-lsystem** (https://crates.io/crates/dcc-lsystem) for the
  rewriting; **bevy-procedural/vegetation**
  (https://github.com/bevy-procedural/vegetation) is early and thin.

### Mesh construction

- **Modeling the Mighty Maple**, Bloomenthal, SIGGRAPH 1985.
  https://www.unchainedgeometry.com/JBloom/pdf/SIG85-ModelingMightyMaple.pdf
  Limbs as generalised cylinders (space curve plus cross-section rings),
  joints blended with implicit surfaces.
- **ProceduralMeshDemos**, https://github.com/SiggiG/ProceduralMeshDemos:
  Catmull-Rom branch paths swept with tube sections, slerped frames,
  sphere joints for welding. Practical and short.
- Leaf cards versus leaf meshes (GPU Gems 3 ch. 4 and SpeedTree docs):
  cards for canopy mass, meshes only where the silhouette needs them; the
  same skeleton yields the reduced LOD by dropping branch levels and
  scaling cards, and the impostor by baking.

### Alien flora, and what makes it read as alien on a normal renderer

- **No Man's Sky**, Innes McKendrick GDC 2017
  https://www.gdcvault.com/play/1024265/Continuous-World-Generation-in-No
  and Sean Murray https://www.gdcvault.com/play/1024514/Building-Worlds-Using
  (https://www.youtube.com/watch?v=C9RyEiEzMiU). Part libraries recombined
  by rules, with palette and silhouette from parametrised functions.
- **Avatar: Frontiers of Pandora** (Snowdrop, above): bioluminescence,
  touch reaction, oversized complementary-colour flora on an ordinary
  scatter system.
- **Astroneer**, https://blog.astroneer.space/p/the-art-of-astroneer-low-poly/:
  a per-planet palette shader over simple curved forms; the world commits
  to the stylisation so botanical accuracy is unnecessary.
- **Subnautica**, https://gdcvault.com/play/1025745/The-Design-of-Subnautica:
  hand-placed, but the recipe (unfamiliar silhouette plus emissive) is
  transferable.
- **Scorn**, https://parametric-architecture.com/environmental-storytelling-of-scorn-a-world-through-hans-ruedi-gigers-eyes/:
  alienness entirely in the modelling language, standard PBR.
- **Journey to the Savage Planet**, https://gamingbolt.com/journey-to-the-savage-planet-interview-design-visuals-environments-and-more:
  saturated, exaggerated silhouettes for readability.

The common thread: the renderer, the placement and the wind are the same
as for an oak. Alienness comes from the generator's grammar (L-system
rules, attraction-point shapes), the palette, emissive accents and scale.
A procedural generator with species parameters in the registry gets alien
species for the price of a second RON file.

## 6. Bevy 0.19.1, verified locally

- **GPU preprocessing and culling** are on by default
  (`GpuPreprocessingMode::Culling`; `NoIndirectDrawing` on a camera drops
  to preprocessing only; DX12 has preprocessing but not GPU culling, WebGL
  neither). Release notes: https://bevy.org/news/bevy-0-19/ (1.6 M mesh
  entities at 53 fps versus 21 fps on 0.18).
- **Automatic instancing**: `Mesh3d` entities sharing mesh and material
  handles batch into one draw with no opt-in
  (`examples/shader/automatic_instancing.rs`); `NoAutomaticBatching`
  opts out. **Custom per-instance data**: `MeshTag(u32)` reaches the
  shader per instance, or a custom pipeline as in
  `examples/shader_advanced/custom_shader_instancing.rs` (this example has
  broken twice between versions, smoke test it).
- **`bevy_camera::visibility::VisibilityRange`**: `start_margin` and
  `end_margin` distance ranges per LOD entity, dithered crossfade in the
  shader (`VISIBILITY_RANGE_DITHER`), checked on the GPU since 0.19; the
  doc example is exactly high mesh, low mesh, billboard impostor, gone.
  `examples/3d/visibility_range.rs`.
- **`OcclusionCulling`** component on the camera: two-phase Hi-Z on the
  GPU, requires `DepthPrepass`, incompatible with deferred, off by default
  and documented as "measure first".
- **Meshlets** (`bevy_pbr::experimental::meshlet`, `meshlet` feature):
  GPU-driven cluster rendering with occlusion and near-seamless LOD, but
  Vulkan and Metal only, needs 64-bit texture atomics, no MSAA, limited
  materials, high base overhead. Interesting for hero trees, not for grass.
- **Third-party crates** are all far behind 0.19: `warbler_grass` (0.13,
  https://github.com/EmiOnGit/warbler_grass), `bevy_procedural_grass`
  (0.12, https://github.com/jadedbay/bevy_procedural_grass, cites the
  Tsushima talk), `bevy_foliage_tool` (0.16,
  https://github.com/ethereumdegen/bevy_foliage_tool, a painting tool with
  chunked instancing and a wind shader), `bevy_feronia` (early),
  `kurtkuehnert/bevy_terrain` (0.14, GitHub only). Useful as reading, not
  as dependencies.

## Open gaps from the research pass

- No public number for Ghost of Tsushima's grass budget or Horizon's
  placement budget outside the recorded talks; watch the videos.
- No primary source quantifying leaf-card overdraw cost.
- No first-party technical source for Breath of the Wild or Genshin grass.
- Whether Bevy's diffuse transmission is adequate for leaf back-lighting,
  and how masked and alpha-to-coverage materials interact with our
  terrain prepass, are things to try, not read about.
