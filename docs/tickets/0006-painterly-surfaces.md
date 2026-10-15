# 0006: Painterly surfaces: the ground, the plants, the built things

Status: parked 2026-10-10. Pass 1 was built the same day and committed
unseen on the GPU (an unattended driver upgrade put the machine on llvmpipe
until a reboot; its record is the "as built" section at the end, and
`scratch/paint-fix/after.sh` is the owed GPU check). The user chose to turn
to content and gameplay; passes 2 to 5 wait. Proposed 2026-10-10, after
ticket 0005 closed (0d0f3c7). Written
from a survey of the shaders and the material graphs and from the look
fixtures and the ticket 0005 stills; nothing built yet. The user asked for
"material shaders to bring the fidelity of our geometry/environment up ...
a painterly guildwars-like look".

## What is wanted

The world's geometry is in: relief, rivers, falls, roads, bridges, sites,
trees, grass, the city. What lets a still down now is the surface of
things. The reference is Guild Wars (1 and 2, and the GW3 trailer that
`docs/research/lighting-fidelity.md` section 1 describes): hand-painted
texture with the light half painted in, broad value shapes rather than
fine grain, a palette per zone, soft open shade, strong silhouettes.
Stylised, not photoreal. The light rig (phases 0 to 6 of the lighting
plan) is done; this ticket is the surfaces under that light.

## What is wrong, seen

From `scratch/look/streaming-repeat-*.png` (the four fixtures) and
`scratch/review/{far,falls,dress}/*.png` (ticket 0005's step 8):

- **The ground is one brown.** The showcase from 120 m
  (`scratch/review/dress/showcase.png`) is bare soil from edge to edge,
  with the trees standing on it like pins. There are 14 layers and a biome
  tint, yet the meadow country reads as mud, not meadow. Fine grain (the
  soil's clods, the sand's ripples at `materials.ron` 12 m repeat) carries
  the surface; there is no colour shape at 20 to 500 m beyond the ±15 %
  brightness of `macro_variation` (terrain.wgsl:210, 210 m and 70 m
  noise), and no hue shift at all.
- **Texture detail at full strength to 40 m.** The stochastic blend is
  variance-preserving (terrain.wgsl:378) so the baked grain reads at its
  full contrast, and the normal map is at full strength to 40 m then fades
  to 400 m (`normal_far`). A painted surface has soft, low normal detail
  and its light painted into the albedo; ours has photographic normals
  under physical light, which is why the sand's ripples shout
  (`scratch/review/dress/r2-lake-reeds-eye.png`).
- **Layer joins are a blend, not an edge.** `height_mix` (terrain.wgsl:485)
  crossfades layers in an 0.08 band; nothing darkens or draws the seam, so
  grass meets soil meets rock as a smear. The bake already has the
  cavity darkening (the `Curvature` nodes) but the runtime drops the
  baked AO (material.rs:800-838 repacks base, roughness, normal, height
  and leaves the ORM's R out).
- **Canopies are an even green.** `foliage.wgsl` lights every leaf card
  the same way: base colour times a per-tree hash tint, baked AO and a sky
  term. The crown's depth and curvature are in the vertex record
  (`shade.z`, `shade.w`, levels.rs:745) and unused. A painted tree has a
  dark core, lit outer clumps, and a rim against the sun. Grass has the
  rim already (`backlight`, grass.wgsl:53) but no colour grouping: every
  blade in a cell draws from one `tint_dry` to `tint_lush` ramp, so a
  meadow has no patches.
- **The built things are clean and muted.** The city's tiles
  (`scratch/look/streaming-repeat-dusk-street.png`) are correct and
  flat. Every building, bridge and roadside surface is a stdlib graph
  recoloured through a `color` param, 63 graphs with no shared palette
  (and settlements pass sRGB where veyra-materials passes linear,
  `veyra-settlements/src/materials.rs:36` against
  `veyra-materials/src/lib.rs:346`). Rocks carry their own painterly
  bake (rocks.rs:1-9) but take no province tint.
- **Water already reads.** The sea and rivers (phase 5 surfaces) are the
  part of the frame closest to the reference. The falls' sheet is glassy
  near (ticket 0005, 8b); that is the only water note.

## What the survey found

The surfaces were drawn for a painterly shader and are lit by a
photographic one. Every veyra graph's module doc says so (terrain.rs:1-19,
flora.rs:1-17, rocks.rs:1-9: shapes a viewer can name, fine noise "to a
whisper", broad light and dark under a third of the repeat). The runtime
then composes them as a plain forward `StandardMaterial` at full
contrast with full normals. So the first move is not new textures; it is
the composition and the lighting of what is baked, and the colour design
over it.

Where the hooks are (file:line as of 0d0f3c7):

- Terrain: `sample_layer` returns at terrain.wgsl:380 (albedo contrast
  line 378); `height_mix` 485; cover and tint 987-992; `macro_variation`
  210 and its multiply 1061; PBR inputs 1095-1123. `TerrainParams`
  (material.rs:241-263) carries `detail_far`, `normal_far`, `tint_far`,
  `cover_far`, `macro_strength`. The repack at material.rs:800-838 is
  where the baked AO could be kept (the normal array's alpha holds the
  height; the diffuse array's alpha the roughness; a third array or a
  packed byte is the cost). Inter-stage locations are full (15 of 16),
  so anything new per vertex must pack into an existing one.
- Foliage: foliage.wgsl:102-123 between `pbr_input_from_standard_material`
  and `apply_pbr_lighting`, with `shade.z` and `shade.w` free;
  tree_impostor.wgsl:446-457 for the far trees (they must match);
  grass.wgsl:191 for the blade colour and grass_common.wgsl:192 for the
  per-blade ramp; `LEAF_LIFT` 2.4 and `BARK_LIFT` 1.5 (tree.rs:74-84) are
  the current, blunt, brightness fix.
- Built things: `ashlar_bevy::standard()` (ashlar-bevy/src/lib.rs:1480)
  is the one place a baked set becomes a material; `export.rs:201-215`
  the one place a graph becomes a `TextureSet`, with planes still in f32;
  `Subgraph` wrapping is a supported pattern, as is `weathering:edge_wear`.
  `AntiTilingExtension` is the one `ExtendedMaterial` on buildings.
- Grading: `RenderLook` (look.rs:119), the per-biome and per-time grade
  (grade.rs:170-195, `assets/biomes/*.biome.ron`, `assets/look/times.ron`),
  the F6 panel, `--render-look`.
- Measuring: `just look --label "<text>"` (four fixtures, GPU mean and p95
  to `docs/perf/look-ledger.md`); `--flight` fps; the terrain's shader
  tests pin layout constants (tests/shaders.rs); no compose test covers
  the water, grass or foliage shaders.

## Two directions, and the recommendation

**A. Paint the inputs, keep the light.** Keep Bevy's PBR and the lighting
plan's rig. Make the albedo carry the painting (broad colour shapes,
value grouped by mass, seams drawn, the baked cavity shade kept), tame
the normals and the roughness so surfaces read matte and soft, give
canopies a core and a rim, give the ground a palette per biome. Every
change sits inside shaders and bakes we own, costs little, and the night,
the fog and the sun keep working.

**B. A painterly lighting model and a paint post pass.** Half-Lambert or
ramp shading, a tinted shadow colour, and an edge-aware smoothing pass
(Kuwahara or bilateral) over the frame to turn grain into strokes. This
is what makes a screenshot look hand-done at once, and it fights the rig
(ramp shading and `apply_pbr_lighting` do not compose; a post pass costs
0.3 to 0.6 ms at full resolution and smears the UI and the far trees'
dither).

Recommendation: A, as the passes below, with the post pass of B tried once
as a `RenderLook` preset (off by default) so the user can compare both in
the F6 panel and the fixtures. Decide after pass 1's stills.

## The passes

Each pass: a workflow (build, review, fix, verify, capture), the four look
fixtures re-captured with a label and the GPU diff recorded, the
`scratch/review/paint/` stills, a section here, a commit by path. The
budget over the ticket is 0.6 ms GPU at the fixtures and nothing on the
sample or the mesher's cost. The ground must not move (GENERATOR_VERSION
17, the pins held).

**Pass 1: the ground's colour and value (terrain).** The most visible
pass, and the only one the user has to judge before the rest.

- Colour shapes: a second macro field with hue, not just brightness: a
  warm/cool and a lush/dry shift at 30 m and 400 m, seeded from the
  existing `value_noise`, applied to the albedo before the biome tint. A
  meadow gets greener hollows and dry crests; a forest floor gets patches.
- The palette per biome: `tint` in `assets/biomes/*.biome.ron` becomes a
  real colour script (the lighting plan's "palette per zone"), with the
  near albedo pulled toward it by a `near_tint` share, not only the far
  one at 400 to 6 000 m.
- Value: lower the stochastic blend's contrast near (the variance line),
  flatten the normal sooner (`normal_far` down, a `normal_near` strength
  under 1), raise `perceptual_roughness` to matte. Keep the baked cavity
  AO through the repack and multiply it into the albedo as painted shade.
- Seams: a darker, narrower band at `height_mix` where two layers meet,
  so grass meets soil with a drawn edge; the rock contact already has one.
- Distance: a touch of desaturation and lift toward the sky's colour from
  2 km, after the biome tint, so the far ground reads as air over paint.
- Captures: the showcase bridge from 120 m, the meadow at eye level at
  the spawn, the forest floor under the canopy fixture, the lake shore
  (`r2-lake-reeds-eye` pose), each before and after; the four fixtures.

**Pass 2: the plants.** Canopy value grouping from `shade.w` (crown
depth: inner cards darker and cooler) and `shade.z` (curvature: outer
clumps lighter), a rim on leaves against the sun on the grass's
`backlight` pattern, the impostor atlas matched (its bake must carry the
same shade or the far trees jump at the swap). Grass: a per-cell hue
patch field (2 to 10 m) between two biome ramps, and the dry blades a
real straw colour. `LEAF_LIFT` and `BARK_LIFT` retired for a proper
exposure of the leaf cards.

**Pass 3: the built things and the rocks.** One palette module shared by
veyra-materials and veyra-settlements (and the sRGB/linear mismatch
fixed), a "paint" `Subgraph` wrapped round every shipped graph at bake
(value compression, a stroke-grain noise, cavity darkening from
`Curvature`, edge wear where it is not already), roughness floors so
nothing shines but glass and water, and the province's `far_tint` on the
rock props as the terrain already does.

**Pass 4: the grade and the study preset.** The biome grade
(`grade.rs`) given the colour script per zone and time (soft greens and
golds, cold blues), a tinted shade (the sky fill's colour per biome), and
direction B's post pass as a `RenderLook` preset behind the F6 panel,
measured, for the comparison.

**Pass 5: the water's painted lines.** Foam as drawn lines at the banks
and the crests (`shore_foam`, `CREST_RAMP`), the falls' sheet given
painted streaks and a matte body, value bands on the sea's depth colour.
Last because the water already reads.

## To decide (the user)

1. Direction A with B as a preset, as recommended, or B outright.
2. The palette: the reference's "soft greens and golds for serenity, deep
   purples and cold blues for foreboding" per biome, or a palette of the
   user's own. Pass 1 ships a first guess per shipped biome for judging.
3. Whether the baked AO is worth a third texture array (one more read per
   layer) or is folded into the albedo at bake (free, but then the
   albedo is darker in the cavities for every light).

## Pass 1: the ground's colour and value, as built (2026-10-10)

Built, reviewed and fixed, but not yet seen on the real renderer. An
unattended upgrade at 06:05 put NVIDIA's userspace at 595.99 over the
loaded 595.91 kernel module, so every game run since falls back to
llvmpipe until a reboot. There are no GPU stills and no GPU timings of
the pass, and the defaults below were tuned by eye on software stills.

### What was built

Colour shapes: `shape_field` is value noise at 420 m and 30 m (12 and 168
cells, both dividing the 5 040 m period), normalised by `SHAPE_SD` 0.16.
It drives a lush/dry hue shift (the noise, hollows from the baked sky
visibility, damp banks) and a warm/cool shift of its own. Both go through
a soft knee (`tanh`) and give the albedo its luminance back, so they are
colour only. `colour_lush` is 1.4 and `colour_warm` 0.75.

A per-layer `palette` flag on `TerrainMaterialDef`, true for grass and
forest_floor, carried as a bitmask in `TerrainParams.palette`: ways,
beds, sand, snow and rock keep their own colour.

The palette lives in a new `BiomeDef.ground`, read but never serialised,
so the `world_version` hash ignores it as it ignores the grade; the
mesher's tint table takes `ground_colour()`. The map keeps `tint`.

Near tint: `near_tint` 0.6 pulls the albedo toward the ratio of the
biome's chroma to the layers' blended mean chroma, `near_value` 0.5 of
that pull also moving value. The vertex stage morphs the tint to the
parent level's mesh (`tint_parent`) and divides out the province factor,
a new record word 15 (`far_factors`), so the near pull aims at the biome's
ground colour and the far tint keeps the province.

Value: `grain` 0.7, `normal_strength` 1.0 to 0.6, `normal_far` 400 to
250 m, `matte` 0.55 per layer (laid and patterned layers keep their own
roughness and contrast). The baked cavity AO is folded into the albedo at
repack (0.8, linear light), answering To decide 3 without a third array.

Seams: `height_weight` split from `height_mix`; the drawn edge is
`(4w(1-w))^3` darkened by `seam_shade` 0.35, faded by `fwidth(w)`.
Distance: from 2 to 12 km a desaturate of 0.25 and a lift of 0.12 toward
the air colour, before the far paint. Painted roads take the same air;
painted water stays unaired to match the river mesh, which hands over well
past 2 km. The F6 panel gains a Ground tab of nine live sliders.

### Where it left the design and why

The brief retinted `tint` in the biome files; that moves the pinned
`world_version`, so the palette is the unserialised `ground` field. The
cleaner home is one line in `strata_worldgen::version` beside the grade's
neutral replacement, left alone because worldgen is closed to this ticket;
a tool that writes biome files back must copy `ground` by hand. The wild
share was the first gate and is zero across carved valleys, hence the
per-layer flag. The meadow pose moved to plains at 22114,-3700 because the
flycam's default spawn is the city street. The showcase is the forest
biome in the sandstone province and reads olive-gold, not green.

### The review's findings and what became of them

Fixed: `SHAPE_SD` (one octave spreads 0.22, the mix 0.159, so 0.16, with a
test); the plateaus from the hard clamp (soft knee, strengths refit);
the unmorphed near tint; the province reddening in the near target; the
far paint air; seam aliasing; the matte on laid patterns; the F6 comments
and two more sliders. Not fixed, because they need the GPU: the forest
floor is still olive-khaki on software, the meadow's value structure is
flat, and every default is unvalidated. The review's first finding stands:
the llvmpipe stills cannot stand in for the renderer, and the lake sand
was already pale with the knobs off.

### The measures

GPU table per fixture, GPU mean / render p95 / main p95 in ms. Before is
the suspect "Pass 1 before" entry in `docs/perf/look-ledger.md`, about
twice the Streaming section and taken minutes before the driver fault.
After was not measured.

| Fixture | Before | After |
| --- | --- | --- |
| canopy | 6.00 / 6.54 / 4.13 | not measured |
| dawn | 6.63 / 7.44 / 4.25 | not measured |
| dusk-street | 7.53 / 7.34 / 5.41 | not measured |
| coast | 7.07 / 4.22 / 3.40 | not measured |

The 0.3 ms budget is unchecked. By reasoning: no new texture reads, added
ALU of four value noises, two hue shifts, a chroma ratio, a `pow` and
`fwidth` per seam, a `tanh` pair, one air mix, and up to six extra storage
words in the vertex stage on odd vertices. The region record grows from 15
to 16 words (slot 253 500 to 270 400 bytes; 1 536 slots 371 to 396 MiB;
about 15 rather than 16 regions fit the 4 MiB upload budget per frame).
Mesher cost per region (`sky_timing`, sum of per-LOD means): 79.83 ms
before, 79.77 ms with the palette table, 79.82 ms with the far factor.

### Tests

Release, one binary at a time, `--test-threads=2`: strata-registry 52 and
1; strata-terrain lib 96, headless 2, shaders 10; strata-atmosphere 90;
strata-client lib 96, ui 11; strata-worldgen `version` 10, including
`the_shipped_world_has_the_pinned_version` at `GENERATOR_VERSION` 17.
`cargo check -p server`, rustfmt, clippy `-D warnings` and rustdoc
`-D warnings` clean on registry, terrain, atmosphere and client. Nothing
changed in worldgen, flora, sim, physics or stream. New tests cover the
uniform against the params field by reflection, the shape spread, the hue
shifts keeping luminance, the cavity shade in linear light, the look
leaving registry fields alone, the ground colour never written, the
tint morph onto the coarse neighbour, and the province factor.

### Captures

Before, on the GPU: `scratch/review/paint/before-{meadow,forest,lake,
showcase}.png`. After, software only: `sw-r4-{showcase,forest,meadow,
lake,dusk}.png` in the same folder, sanity checks and not evidence. The
canonical after stills `{meadow,forest,lake,showcase}.png` do not exist.

Judged 2026-10-10 from `scratch/review/paint/sheet.png` (four pairs, GPU
before on the left, llvmpipe `sw-r4` after on the right, each pair 1280
wide). The GPU is still down, so no pose could be retaken on it.
`before-meadow`: plains at eye level, grainy dark green with a near black
hillside; `sw-r4-meadow`: the same ground matte, lighter and yellower green
with broad patches, the hill now lit. `before-forest`: red-brown leaf litter
at full grain; `sw-r4-forest`: the same litter turned olive-khaki and
quieter, still not a hand-painted floor. `before-lake`: grey sand with
ripples that shout; `sw-r4-lake`: warm pale matte sand, ripples calmer.
`before-showcase`: brown from edge to edge with the river and road as
dark lines; `sw-r4-showcase`: olive-green and gold with lighter broad
patches, river and road still read. `sw-r1-showcase`, `sw-r2-*` and
`sw-r3-*` are the earlier rounds (r1 unchanged, r2 too weak), `sw-r4-dusk`
the low-sun sheen on the city's flagstones, and `sw-debug-showcase` the
wild share, green slope and forest_floor debug view. `sheet.png` is the
one to open first.

### What is left

After a reboot run `scratch/paint-fix/after.sh`: it rebuilds HEAD in a
worktree, reruns "Pass 1 before" in the same session, takes the four GPU
stills, runs "Pass 1 fixed" and prints the compare. Then judge and retune
the defaults on those stills, check the seams and the near tint in motion
(a flight and a walk along a road), and replace the suspect ledger entry.
Cavity and the air terms are not on F6. The user's palette (To decide 2)
is still a first guess.
