# 0002: A settlement drawn as a hierarchy, to the horizon

Status: steps 0 to 3 done and measured, 2026-09-29 (see "Built" below and
the ledger's section of that date); steps 4 to 6 open. Follows the ledger's
sections of 2026-09-27 and 2026-09-28, which ended on diminishing returns
from per-piece changes.

## Problem

Two things, with one cause.

1. **Past 3 km the city is not there.** `apps/game/src/settlement.rs` spawns a
   settlement's buildings within 2.5 km of its centre and despawns them past
   3 km. The terrain draws to 48 km (`StreamConfig::view_distance`), so from a
   ridge 5 km off the bay is bare ground.
2. **Inside 2.5 km the city's far level is as many entities as its near
   ones.** The bake uses ashlar's default ladder (`LodPolicy::ladder()`, tuned
   for the showcase at 1080p): three levels, the last from 250 m to any
   distance. A level simplifies triangles and leaves the piece count almost
   alone, and the render thread pays per entity in band, about 0.5 ms a
   thousand (ledger, 2026-09-27).

From `generated/stats/buildings.ron`, the city's 32 files (each placed once):

| Level | Band | Pieces | Drawn triangles | Largest shares of the pieces |
| --- | --- | --- | --- | --- |
| 0 | to 60 m | 15 072 | 7.5 M | houses 8 950, walls 3 710 |
| 1 | 60 to 250 m | 13 151 | 2.1 M | houses 8 434, walls 2 804 |
| 2 | 250 m on | 7 272 | 0.45 M | houses 3 323, walls 2 573 |

Level 2 is cheap in triangles and dear in entities, and it is what the bay
and the approach look at. Its smallest feature, 1.5 m, is about half a pixel
at 2.5 km, so most of those 7 272 pieces draw nothing anyone can see.

The cause of both: a building's levels are all the same kind of thing (placed
pieces under one root), so there is no level cheap enough to keep to the
horizon and none that costs fewer entities than the one before it.

## Principle

The entities in play from any one place should be about the same number
whatever the distance to them. Near, a piece is an entity, because it is
shared, instanced and culled on its own. Each ring further out is an
aggregate of the ring inside it, made in the content step, so a ring costs
entities by its area in cells and not by what stands in it. This is what the
far trees already do (a batch per cell and species, ADR 0005) and what
ticket 0001 plans for the near trees.

## Design

| Ring | Band | What draws | Entities, whole city |
| --- | --- | --- | --- |
| Pieces, level 0 | to 60 m | as today, streamed by 40 m cell | those in reach |
| Pieces, level 1 | 60 to 250 m | as today, streamed by cell | those in reach |
| Cell proxies | 250 m to about 1.2 km | level 2's geometry, concatenated per 80 m cell and material | cells x materials, a few hundred |
| Skyline | about 1.2 km to the horizon | the whole settlement at a coarser policy, one mesh lit and one unlit | 2 |

**Cell proxies.** The content step takes each building's level 2 as it is
meshed today, places every piece, and concatenates the triangles by cell and
by material binding. No boolean union: this is not ashlar's merge group, it
is a copy of vertices. The file format may not need to change, since a
`BakedLevel` already carries `groups` of batches by binding, drawn about the
centre of their bounds; whether `ashlar-bevy` draws groups of a building that
is not `merged` is the first thing to check. It belongs in ashlar
(`LodPolicy` gains a cell size, `None` leaving a level as pieces), because
every game with a town has this problem.

Instancing is lost in that ring, which is its price: level 2 drawn is
451 000 triangles, about 35 MB of vertices against 12 MB unique today. Level
1 merged the same way would be 2.1 M triangles and some 170 MB, which is why
level 1 stays pieces.

**Skyline.** One more policy over the whole settlement (nothing under 4 m to
start with, to be tuned by eye), every building placed in the settlement's
frame, written as `generated/settlements/<id>.skyline.ashlar` and named in
the layout. Vertex colours from each material's mean albedo in place of
textures, so it is one material: past 1.2 km a metre of facade is a pixel.
Elements marked `far` (lamps, warning lights, lit panes) go in a second mesh
under an emissive material, so `night_lights` dims the distant city with the
near one. Target: under 40 000 triangles, a few megabytes.

The game spawns the skyline while the settlement is inside the terrain's view
distance, and the buildings inside about 1.6 km instead of 2.5, since the
skyline now covers the approach. The two hand over with the crossfade the
levels use.

**Shadows.** Proxies and skyline cast; pieces cast inside their own band.
The shadows that read from the bay (the transit deck's, the walls', the
stairs') then come from a few hundred casters, not from thousands.

**A budget that fails a test.** `tools/content` already holds each building
to `budget::PIECES_PER_LEVEL`. Add one for the settlement: entities in play
from the bench's city viewpoints, computed from the layout and the bands
with no renderer, so a kit that adds ten thousand pieces fails in
`just test` and not in the ledger.

## Decided

2026-09-28: the cell proxies are ashlar's (a cell size on `LodPolicy`), and
the skyline is a mesh, not an impostor: a city a kilometre wide seen from
1.2 km has more parallax than an atlas of frames holds, and a mesh takes the
real sun and haze. Whether the planted plants join this ticket is open until
step 0 has their share beside the city's.

## Measured at step 0 (2026-09-28)

`cargo run --release -p content --example census`, over the baked files:

| Level | Pieces | Bindings | Batches at 40 m cells | 80 m | 160 m |
| --- | --- | --- | --- | --- | --- |
| 0 | 15 072 | 58 | 3 919 | 1 670 | 628 |
| 1 | 13 151 | 57 | 3 706 | 1 620 | 620 |
| 2 | 7 272 | 45 | 1 952 | 851 | 350 |

Bindings are tens, so a batch per cell and binding works without moving
colour into the vertices. "A few hundred" was optimistic at 80 m: it is 851,
an eighth of the pieces, and 350 at 160 m.

Bevy's shadow views honour `VisibilityRange` (`bevy_light` 0.19.1 asks
`entity_is_in_range_of_view` of the camera's view), so a piece out of its
band casts nothing already.

Two corrections to the design above, from reading the code:

- **The skyline keeps its materials.** A baked building refuses vertex
  colours (`no_vertex_data`, ashlar ADR 0007), so the first skyline is a
  batch per binding, some forty entities for the city, under the materials
  the near city wears. `night_lights` then dims it with no second mesh.
  Vertex colours are a later step if loading forty materials for a city of
  twenty pixels shows in a measurement.
- **The skyline and the buildings hand over by the game, not by range.** A
  range is measured to each entity's origin: the proxies' to their cells, the
  skyline's to the city's middle. At any one distance some cells would be
  drawn twice (two coplanar walls) or not at all. So a settlement is in one
  of two states. Far, only the skyline draws. Near, the buildings draw, their
  last level to any distance, and the skyline is hidden. The buildings
  stream in hidden while the observer is still further than the switch, and
  the switch is made for the whole city at once, where what the skyline's
  policy drops is under two pixels. That also ends the buildings arriving
  one a frame in view.

## Steps

Each measured with `just bench --xvfb`, interleaved A/B, before the next.

0. **Measure what the design assumes.** Distinct material bindings in level
   2, per file (houses carry colours per instance, and a proxy per cell and
   binding is only cheap if bindings are tens, not hundreds). Pieces in band
   by level at city-bay, city-terraces and city-approach. Whether shadow
   views honour `VisibilityRange`.
1. **Upper bound first.** Despawn every level 2 piece in a throwaway build
   and bench: that is the most the cell proxies can win. The plants' far
   levels were measured this way on 2026-09-28. Stop here if it is under
   1 ms of render time at city-bay.
2. **Skyline.** Content step, layout entry, spawn by view distance. It costs
   nothing measurable and fixes the missing city, so it does not wait on
   step 1. Checked by captures from 2, 5, 15 and 40 km at noon and at night.
3. **Cell proxies** in ashlar and the content step; level 2 pieces no longer
   spawned. Ashlar's own `just ci` first.
4. **Tune the bands** with the ring structure in place: a ladder of the
   game's own for 1440p, and whether level 1 can end nearer than 250 m now
   that what follows it is cheap.
5. **Level 1**, only if steps 3 and 4 leave the city over the gate: the
   houses' level 1 is 8 434 pieces and the next largest cost. Candidates are
   merging each house's elements per material within a cell (memory against
   entities, to be measured) or a nearer end to its band.
6. **The budget test**, then the ledger, then an ADR amendment (0009) for the
   skyline in the layout.

## Built (2026-09-29)

Steps 2 and 3, ahead of step 1's upper bound; ADR 0009's amendment has the
design as built.

- **Cell proxies**: ashlar's `LodCell` (its ADR 0006 amendment), the game's
  ladder `content::ladder()` joining the last level into 80 m cells. The
  city's far level is 1 040 batches for the 7 272 pieces it was. 160 m is
  not measured.
- **Skyline**: `generated/settlements/port_serein.skyline.ashlar`, named by
  the layout's `skyline`, 37 batches (35 before it kept the lit
  elements). Its root is hidden until its pieces are in, and then shown
  exactly when the buildings are not, so a skyline finishing its load
  while the buildings draw is never drawn over them. The game switches the whole city at
  1.7 km from its edge, not the 1.6 km above (distances in ADR 0009). At the
  game's 45 degrees and 1440 lines 4 m is about four pixels there, not two:
  two would put the switch at 3.4 km. The skyline keeps long elements down
  to 1.5 m wide, or the processor's pipelines appeared at the switch.
- **The skyline keeps what is lit.** The first night captures had the
  city's lights come on at the switch: 2 pixels over 140/255 in a crop at
  2 km (far) against 305 at 1.5 km (near). An audit per source building
  of every element bound to a glowing definition found the lamps, floods
  and neon all marked `far` and kept, and two losses by `prepared`: the
  housing slabs' lit slits are members of a large element the skyline
  kept at 4 m and so lost (108 of 768 m2 of lit surface at slab-0), and
  the market's shop fronts and panes likewise (11 of 406 m2); the
  Terraces' house glass is dropped by the farthest level too, so by
  neither. `prepared` now keeps every element whose binding's definition
  glows (a positive `emissive`, for any instance placing it) as the
  farthest level keeps it (`skyline::FARTHEST_FEATURE`, 1.5 m, with a
  test holding it to `content::ladder()`), decided by the material and
  not by `far`, which would keep a slab's hairline members too. A content
  test holds each building's lit area in the skyline to at least 90 % of
  its farthest level's (every one is now 100 %). Captured at night from
  the same place (1 274 m from the edge) the skyline, forced, has 300
  pixels over 140 and the buildings 292; at 1 774 m, where the city is
  far, the skyline has 14, the same windows a third further off, most of
  each slit under a pixel (`scratch/review/far-detail-fix2/`).
- **A far city's pieces are out of play.** While a settlement is far every
  group of its buildings' pieces is `Disabled`, in band or not: at 1 774 m
  the census went from 1 040 pieces in play under hidden roots to none.
  They come back on the frame the city goes near, before that frame's
  propagation (the order is in `install`). Flown in at 200 m/s under
  xvfb, the switch frame's work was within the run's usual spread (render
  18.3 ms worst against a median of 11.0, main 13.5 against 7.1, wall
  88 ms against 71) and no pre-enabling was needed. The long frames of
  the approach are the buildings spawning (a 0.75 s main frame for the
  Terraces' houses, 0.3 s for the walls, the switch waiting on the last
  of them), before and after this change alike.
- **Missed: the skyline's triangles.** 180 973 against the target of
  40 000 (249 153 before its chamfered boxes were made plain, 174 849
  before it kept the lit elements). What is left is boxes: the Terraces'
  house trim (about 41 000) and walls (32 000) and the terrace walls'
  masses (27 000), kept because ashlar judges a thin broad slab by its
  broad side. `budget::SKYLINE_TRIANGLES` is a ceiling of 181 000 over
  the measured number until one of: a far variant of the house kit, a
  change to ashlar's rule for thin broad slabs, or vertex colours and a
  decimation of the joined mesh.
- **Measured** (the ledger's section of 2026-09-29): the render thread
  gains 0.6 ms in the bay, 1.2 on the approach and 1.9 from 2 km, nothing
  among the terraces. 160 m cells (482 batches) measure the same as 80 m
  (1 040), so 80 m stays. The bound of step 1, nothing of the city past
  250 m, is a millisecond further on, and that millisecond is drawing the
  far city at all.
- **Captured** from 0.3, 1.5, 2, 5, 15 and 40 km at noon and from 1.5, 2
  and 5 km at night (`scratch/review/far-detail*`): the city is there at
  every distance, nothing floats or is drawn twice at the sizes a still
  shows, and the slabs' lit windows are the same either side of the
  switch. Shimmer and the switch itself in motion have not been watched by
  a person.
- **Open**:
  - the skyline's triangles, above;
  - steps 4 and 5: the city's views are still over the gate, and what they
    pay for is levels 0 and 1 in band and the planted plants;
  - the budget test of step 6 as entities in play from the bench's
    viewpoints (what exists holds the far level's and the skyline's
    pieces);
  - the far city at night is dim: the houses' glass, the starport's pads
    and the transit's lamps are dropped by the farthest level too, and
    nothing of the city passes for a light at 5 km. Ticket 0003 takes
    lights at a distance up with the fog;
  - the haze and coarse terrain risks below were looked at in stills only.

## Risks

- **Bindings per cell.** If the houses' per-instance colours make hundreds
  of bindings, proxies need the colour moved into a vertex attribute so a
  cell is one material. Step 0 says.
- **Coarse terrain under a fine skyline.** The city's ground is held at
  terrain level 0 only near it. From 20 km the terraces are a few coarse
  triangles and a building may float or sink by metres. The skyline should
  carry the terraces and walls (they are buildings already) and extend every
  base downwards a few metres.
- **Haze.** The skyline has to take the same aerial perspective as the
  terrain behind it, or it reads as a cut-out. Check how
  `strata-atmosphere` reaches a `StandardMaterial` at 20 km.
- **Handover popping.** A proxy switches on its cell's distance, the pieces
  under it on their own. The crossfade margin hides a level change of one
  piece; whether it hides a cell of them is a capture, not a guess.
- **Bake time and download.** About 25 MB more on disk, and the settlement
  is meshed once more for the skyline.

## Not in this ticket

Planted plants (3.7 entities each, the largest single share at
city-terraces) are foliage and belong to ticket 0001, step 4: the same
pattern, a different crate.
