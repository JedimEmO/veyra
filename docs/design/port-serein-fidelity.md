# Port Serein: the fidelity passes

Port Serein reads as a city, but not yet at the quality of the rest of the
game. The trees and the foliage are the bar: generated, but with dedicated
graphs baked at 1024, several authored levels, impostors, wind and per-plant
variation. The corporate kit in `veyra-buildings` is the bar for
architecture. This plan takes the city there in five passes.

## Where the gap is

Four problems, each with a cause found in the captures and the code.

**Surfaces fight each other and the terrain.** Some of it we author. The sea
wall's top sits at exactly the quay's level, and so does the terrain. Each
retaining wall's top is flush with the terrace above, and so is the terrain.
Where a stair cuts a wall, the terrain's own bank shows through the gap. The
rest is the terrain's resolution. Its mesh is a 65-by-65 grid per region: 1 m
between vertices at L0, doubling with every level. Most of the city is drawn
at L2 to L4, 4 to 16 m between vertices, laid over terrace banks about 3 m
wide. A coarse triangle spanning a bank interpolates straight through the
wall and the floor in front of it, and morphing between levels moves it
again as the camera moves.

**Textures tile.** Every building material is a plain `StandardMaterial`
sampling a baked tile, and the graphs repeat every 2 to 4 m. A hundred-metre
retaining wall shows its tile fifty times, and nothing breaks the pattern:
no macro variation, no stochastic sampling, no per-instance offset. The
terrain shader already solves this for the ground. It samples in rotated
triangular cells and adds a slow world-space albedo variation. The
buildings have neither. A few of our definitions also stretch their graph
(`tile_scale` of 2 on the paving and the composite), and the close surfaces
bake at 512.

**The kits have one level of relief.** The corporate annex has five at the
5 to 50 cm scale: frames standing proud of the wall, recessed glazing with
mullions, cornices with a real overhang, a plinth, grilled plant. A terrace
house has a box, a hole and a thin frame. The detail is also paid for
wrongly. Every house is its own shape, so the houses are about 630 000
unique triangles and share almost nothing. The corporate kit is a few
richly detailed parts, reused.

**Nothing grounds a building.** Walls meet gravel along a hard line: no
paving, no kerb, no soil, no weeds, and no contact shading (the baseline
look has no ambient occlusion). The glass is a constant colour with nothing
behind it.

## Pass 0: gates first

Two gates make the first two problems testable before they are fixed.

1. **A coplanar-face check** over every baked building, in the content
   step's tests: two triangles on one plane (within a millimetre) that
   overlap, in different materials or facing the same way, fail with their
   element paths. This belongs in ashlar as a validation over meshed levels,
   so every game gets it.
2. **A terrain clearance check** for the city: for each LOD from 0 to 4,
   interpolate the terrain the way its mesh does (the same grid, the same
   triangles) over the city's owned ground. Assert that it stays a margin
   below every walkable floor, wall top and building base, or inside a solid
   volume.
3. **A review rig.** The city's kits go into `just review`'s galleries beside
   the corporate references, from fixed close, medium and far angles, by day
   and by night. Each pass ends with a Codex critique against the trees and
   the corporate kit, and with `just bench --xvfb`.

## Pass 1: the city is solid

The fix for fighting with the terrain: the terrain goes under the city,
and the city brings its own floors.

- **Sink the terrain.** Inside the owned ground, worldgen sinks the terrain
  about 1.5 m below the terraced height. At the extent's edge it rises back
  behind a retaining kerb. The terrain becomes an underlayer nobody sees.
- **Floors as geometry.** Every level run gets a floor slab 1.6 m thick,
  traced along the terrace like the walls. Its surface is a real one: paved
  lanes with kerbs and gutters, soil beds in the gardens, the quay's concrete
  apron. A coarse terrain triangle now stays inside a solid at any LOD.
- **Walls deepen** to below the sunk terrain, and are rebuilt to abut the
  floors rather than share their plane. The floor stands 1 cm proud where
  the two overlap.
- **The coplanar cases we author go.** The sea wall's top goes under the
  coping, and so does every case the Pass 0 check reports.

This also retires the gravel texture as the city's ground, which is half the
"not our quality" look at street level.

## Pass 2: textures that do not tile

- **Anti-tiling in ashlar-bevy.** A `MaterialExtension` over
  `StandardMaterial` that brings the terrain's two techniques to buildings.
  First, stochastic sampling (rotated triangular cells, blended), which
  breaks a tile's repeat. Second, a world-space macro variation of albedo
  and roughness, a few metres to a few hundred across, which breaks
  uniformity. It is opt-in per definition through a new field on
  `MaterialDefinition`, off for glass, trim and signs. It lives in ashlar,
  with its own `just ci`.
- **Our own hero graphs at 1024.** The render and stucco has grime rising
  from the base and streaks under sills. The poured concrete has its form-tie
  grid sized to the storey. The terrace walls get block masonry, the lanes
  setts or slabs sized to the lane, the composite ribbed panels. Each graph's
  period is chosen to land on the architecture (a storey, a bay), so its
  seams fall on geometry. The graphs can read height because UVs are in
  metres.
- **UV discipline.** Walls and floors use `UvMode::Box`, so neighbouring
  pieces continue one pattern. Each instance gets a `uv_offset` jitter, so
  neighbouring houses do not show the same patch. The `tile_scale`
  stretches go.
- **Glass with an inside.** Panes take the library's office-window and
  window-band graphs (blinds, curtains, a lit or dark interior) instead of a
  constant.

## Pass 3: kits at the corporate bar

- **A catalogue of parts, not a unique shape per house.** A house is five to
  ten instances of shared, detailed parts: a ground module, an upper module,
  a roof, a stair, a balcony, a shed. Its variety comes from the choice and
  arrangement of parts, the paint, mirroring and the offsets. Detail per
  mesh goes up, and unique triangles go down.
- **Assemblies** reused across the types:
  - a window: frame, reveal, sill, lintel, glass and louvred shutters;
  - a door: frame, panelled leaf and step;
  - cornices and eaves with real overhang, a string course, a plinth, and
    parapet coping;
  - balconies with railings;
  - downpipes, gutters and cables;
  - AC units and meters.

  Small detail sits in its own elements, so level 1 (past 60 m) drops it.
- **The ashlar extensions** this needs, from the style doc: sweeps first
  (rails, pipes, cables, gutters), then drapes (stall cloth, awnings).
- **Then each type in turn:**
  - the Landing modules: panel seams, porthole frames, hatch doors;
  - the slabs and the headquarters: facade modules like the corporate kit's,
    access galleries and stair towers;
  - the warehouses: ribbing, doors, gutters;
  - the terminal: a mullioned curtain wall and fins;
  - the processor: crowns and intakes at the tower mouths, walkways and
    ladders;
  - the stalls: draped cloth, and goods in variety.

**The terrace house is done (2026-09-27)**, as `veyra_settlements::types::house`.
A house is a ground module (6, 8 or 10 m wide, 6, 8 or 10 m deep, one or
two storeys, shuttered or casement), perhaps an upper module (4, 6 or 8 by
4 or 6 m) stood against the back of its roof, a vault or a lean-to over the
top, and add-ons (balcony, awning, shed, tank, air conditioner, dish,
panels, washing, stair hutch, aerial, planters); a workshop is a module of
its own. The plan draws its lots from those sizes. Each module is a solid
block in four elements: painted walls with recessed openings and sunk side
panels; trim (plinth, string courses, a two-step cornice, pilasters,
coping, sills, hoods, box frames, door surrounds, canopies); painted
joinery (frames in bars, louvred shutters, panelled or flush doors, rails,
downpipes); glass. Walls, trim, joinery, glass and cloth are bound per
house. The Terraces went from 832 parts to 88, and from 652 000, 516 000
and 199 000 unique triangles by level to 252 000, 63 000 and 12 000 (a 90 MB
file to 26 MB). The price is entities and near triangles: 9 080 placed
elements instead of 4 658, and about 8 500 triangles drawn per house within
60 m instead of about 800. `building-preview` has the catalogue, a street
and single houses as scenes (`serein-*`) for the gallery.

## Pass 4: grounding and light

- **Contact shading.** Ambient occlusion (SSAO, with TAA instead of MSAA)
  or contact shadows in the baseline look. In the lighting contact sheets it
  tied the buildings to the ground more than any geometry change. It costs
  about a millisecond.
- **The foliage at the city's feet.** The foliage system is our best-looking
  system, so the city should use it. The plan marks soil beds and wall feet
  as plantable ground, and grass, weeds and shrubs grow there from the same
  placement as the wilderness.
- **Transitions as geometry:** skirting strips of grime at wall bases,
  gutters at lane edges, worn thresholds.

**The rest of the kits are dressed (2026-09-27).** None of them is a
catalogue yet: there are about twenty modules and ten warehouses, so detail
per building mattered more than sharing.

- **Landing modules** (`types/landing.rs`) have a hatch door in a frame,
  with its leaf, a step and a lamp. A trading module has a glazed shopfront
  with mullions, a kick plate and a shutter box. Every porthole has a hood.
  The back carries conduits, a meter box and an air conditioner; a ladder
  runs up one side, and the roof has a railing and a mast.
- **Warehouses** are loading docks. Each bay has a steel frame, a shutter
  box and a roller shutter, some rolled half up, with dock bumpers and hazard
  posts. There are portal-frame pilasters, gutters and downpipes, a ridge
  cap, rooflights, gable louvres and floodlights.
- **Housing slabs** have a front grid in a paler cast concrete: fins and
  floor bands with the windows deep between them. Access galleries on
  brackets run along the back. The stair core has a glazed slot and the
  lobby's glass; the roof has a parapet, plant, tanks and masts.
- **The terminal's control tower** has steel rings, a railed catwalk and a
  mullioned cab.
- **Retaining walls** are coursed limestone on a concrete plinth, under a
  coping, with drains. A wall 6 m or taller steps back onto a planted
  ledge.
- **Every stair** has handrails.
- **Window glass** is the library's office window, box-mapped, so each pane
  shows its own part of a room.

Later the same day:

- **The stalls** have cloth that sags between its frame bars, a scalloped
  valance, a pleated counter skirt, and goods: fruit, greens, tins and
  hanging bunches. They are built from extrusions and hulls, with no
  drape support in ashlar.
- **The headquarters** has a platform of its own in the Heights. It never
  found a lot before. It has a colonnade of fins over glazed executive
  floors, a deep crown with teal light, and a canopy at the entrance.
- **The terminal's podium** has a coping, pilasters, a plinth, a teal strip,
  and handrails on its grand stair.
- **The processor's gantries** are trusses.
- **Four housing slabs and nine warehouses** stand now, where before there
  were two slabs and one warehouse. The slabs slide along their terrace to
  find room, and the warehouses back onto the terrace behind the quay.
- **The terrace walls' stone** is a warm limestone. ashlar's
  `library:ashlar-blocks` now takes a `color`.
- **The old metropolis plan is retired**, and with it 27 materials that
  nothing bound.

Still open: nothing from this plan's passes, except the budgets below.
Performance is deferred. The planted plants (about 2,400) and the new detail
have not been measured against the render gate.

**The ground has no seams (2026-09-27).** The terrain drew two layers per
triangle, read from its first vertex. That drew every diagonal border
between laid materials as a sawtooth, and every meeting of three as a fan of
teeth. Each vertex now carries the share of all sixteen layers, tent-filtered
over the sampled grid. The shader blends the three strongest by height, so a
border runs where two shares cross.

**The walls' feet are planted.** Ferns, wildflowers and scrub from the flora
library grow in clumps at the walls' feet, wherever no house, stair or kit
stands.

## Budgets

The style doc's budgets hold: about 8 000 pieces and 1.5 M triangles in
view. Pass 3 should lower the unique triangles (shared parts), while
triangles drawn may rise near the camera, where the fine levels draw. Each
pass records `just bench --xvfb` for the city fixtures in the ledger.

## Decided (2026-09-27)

- Anti-tiling goes into ashlar-bevy as an opt-in material extension.
- Houses and the other kits become catalogues of shared, detailed parts.
- SSAO with TAA joins the baseline look (Pass 4, or earlier if it helps
  judge the passes before it).

## Order

Pass 0 and Pass 1 first: they fix the visible bugs, and the floors change
what every later pass stands on. Pass 2 next: it is the cheapest large
improvement, and it serves every existing kit. Then Pass 3 type by type, the
terrace house first, because it is most of every view. Then Pass 4.

## Neighbourhood benches and port fittings, 2026-10-01

The supplied hillside and flycam captures show continuous bare contour
bands, reinforced by parallel house rows. At the Exchange, the pale paving
has little to establish a street edge; the night view lacks small pools of
warm light. Before captures, reduced to 1024 px with `xtask`:
[hillside](city-before-hillside.png), [flycam](city-before-flycam.png),
[street](city-before-street.png), [night](city-before-night.png).

The ground now cuts finite neighbourhood leases, with unequal widths,
slightly different levels and occasional omitted steps that merge benches.
Their ends feather into natural gullies, and the outer hillside keeps its
uncut slope. Rounded bank crests and toes vary in width. Retaining walls
stop at gentle banks; scrub and stone from the existing flora library soften
their ends. Platforms, quay and the road pass retain their levels.

Twelve small courts reserve cross-lanes before the houses are fitted.
Short clusters turn a few degrees, and infill on deeper benches turns across
the contour. Landing modules extend into the old quarter; six Pelagic slabs
occupy the heights. The final plan has 363 terrace houses, against 835 before,
and 621 retaining-wall pieces against 1,075. This is a substantial density
reduction, deliberately leaving gullies and varied open patches. Split-level
houses are deferred: these changes keep the existing types on verified flats
and give the first visual review a simpler grounding contract.

Nine shared recipes in `veyra-buildings::port` adapt the useful parts of
ashlar's city and sci-fi kits: steel window reveals, lit shop fascias,
chamfered corner lights, downpipes and caged service lamps, water blade signs,
kerbs with dark drain channels, public terminals, a waiting shelter and
market lantern strings. The placement pass adds 183 fittings around existing
shops, courts and the Exchange. Existing building types and the light Landing,
coloured Terrace and heavy Pelagic palettes remain. No new material graph or
texture set is introduced.

GPU-free comparison: [plan before](city/plan-before.png) and
[first-pass plan](city/plan-pass1.png). These show actual traced benches and levels,
lot turns, courts, lanes and stairs; walls and landmarks use plan envelopes.
The shorter bench runs and sideways infill are visible in the after plot.
Regenerate with `cargo run --release -p content --example plan --
docs/design/city/plan-after` (Python, NumPy and Matplotlib). The compressed
TSVs preserve both states; `plot_plan.py` also reads those directly.

Two cached content bakes followed the in-memory layout and meshing checks:
7.9 seconds for the city, then 3.7 seconds to refresh fitting anchors. Both
reused all 82 texture sets and wrote 110 definitions. Whole-city census,
not simultaneous screen cost:

| Measurement | Before | After |
|---|---:|---:|
| Near pieces / drawn triangles | 15,072 / 7,537,419 | 9,040 / 3,540,262 |
| Middle pieces / drawn triangles | 13,151 / 2,092,327 | 7,665 / 1,056,578 |
| Far pieces / drawn triangles | 1,040 / 450,937 | 1,007 / 255,495 |
| Near material bindings | 58 | 58 |
| Far material bindings | 45 | 46 |
| Skyline pieces / triangles | 37 / 180,973 | 38 / 116,333 |
| Skyline file | 17.1 MB | 11.0 MB |

Sources: [before census](city/census-before.txt),
[first-pass census](city/census-pass1.txt), [bake log](city/bake-pass1.txt).
The far ceiling remains 1,200 pieces, and the skyline ceilings remain 64
pieces and 181,000 triangles. Ticket 0002's 40,000-triangle target is still
open. The ground revision is 2 and generator version is 11; the pinned
world checks were updated for the intentional settlement change. Spawn and
lobby remain `[-40, 38, 62]`, with ground and collider clearance checks.

Validation passed: formatting, clippy on all touched crates with warnings
fatal, the content and census gates, and `just ci`, including the separate
headless build and tests. Checks used `CC=clang HOST_CC=clang`, two Cargo
jobs and two test threads. No ashlar source was changed.

At this pass no game was launched in the GPU-free sandbox. The next review
was to compare matching hillside, flycam,
street and night captures, focusing on remaining wall dominance, density,
fitting alignment and lighting intensity, then measure in-view render cost.


## Second layout and Exchange pass, 2026-10-01

The returned captures (`city/after-street.png`, `after-flycam-street.png`,
`after-hillside.png` and the subsequently recovered `after-night.png`) show
that the finite benches removed the hillside stripes, but 363 houses left
whole benches empty. The Exchange foreground remained pale paving by day
and dark paving by night. Those captures are the first-pass result, not
screenshots of this second pass. No game was run here.

The ground, gullies, cross-lanes, twelve courts, public spawn, lobby, quay
and road exit are unchanged. The ground fingerprint remains
`5f2b67d675798988`. The changes are in placement and reusable recipes:

- **762 dwellings: 714 ordinary houses and 48 split-level houses.** Narrower
  frontages, smaller side gaps and longer clusters fill the usable runs up
  to the existing interruptions. Random gardens thin the ridge and outer
  edges; the middle slope and quay neighbourhoods keep their frontage.
  Wide benches carry pairs with joined backs and doors onto lanes on either
  side. Small contour-relative turns remain, within sampled flat ground.
- **Split-level houses use their lower floor as the retaining structure.**
  Its downhill door opens onto the lower bench; the existing house kit sits
  on the upper deck with its entrance facing the upper bench. Both entry
  approaches are sampled across their width. The buried back replaces the
  wall locally: 556 retaining pieces, down from 621. These are exterior
  dwellings with the same solid collision convention as the existing kit.
- **4,553 plants and rocks**, up from 1,629. Larger scrub and stones follow
  clear bank feet and crests at roughly 3.5 m intervals, plus a 7 m scatter
  on unbuilt slopes. Dense house runs cover part of the banks; exposed
  shoulders carry visible clumps instead of isolated, low ferns. Flora uses
  the existing library and distance levels, with no new species or textures.
- **The Exchange arrival is dressed in the actual spawn view.** Dark masonry
  footways, drains and kerbs define the canteen and clinic frontages. Four
  additional post lamps, six bollards, two planted beds, two benches, a
  nearby terminal and crate stacks flank the approach. Three additional
  lantern spans frame the nearest market aisle. Stall fascia boards carry
  task lights, the clinic's visible side has a projecting sign and downlight,
  and its portholes bind warm interior glass. Point lights accompany lamps
  and lanterns so the square receives illumination as well as emissive marks.
  The eight-metre stair approach remains clear.

Review plots generated from the shipping placement pass:

- `city/plan-before.png`: original 835-house city.
- `city/plan-pass1.png`: the 363-house first pass.
- `city/plan-after.png`: this 762-house pass, reviewed before dressing and
  refreshed with the final furniture placements.
- `city/plan-after-exchange.png`: closer review of the arrival furniture,
  paving edges, market lanes and clear route from the spawn.

The plots show sustained frontage between the gullies, transverse bank
houses and denser paired infill on broad benches. They establish placement;
lighting, terrain LOD intersections and the larger planting's appearance
still need the external game captures.

Final census, pieces / distinct bindings / drawn triangles:

| Level | Original | First pass | Second pass |
| --- | --- | --- | --- |
| Near | 15,072 / 58 / 7,537,419 | 9,040 / 58 / 3,540,262 | 13,411 / 58 / 6,032,639 |
| Middle | 13,151 / 57 / 2,092,327 | 7,665 / 58 / 1,056,578 | 11,721 / 58 / 1,822,117 |
| Far | 1,040 / 45 / 450,937 | 1,007 / 46 / 255,495 | 1,099 / 46 / 390,413 |
| Skyline | 37 / 37 / 180,973 | 38 / 38 / 116,333 | 38 / 38 / 156,185 |

All four triangle totals remain below the original bake. Far pieces are
higher than the original but below the unchanged 1,200 gate; skyline pieces
and triangles remain below 64 and 181,000. The ticket's longer-term 40,000
skyline target is still open. The census counts buildings, not foliage;
the denser bank planting needs a runtime performance check in the captures.
The skyline is 14.2 MB, versus 17.1 MB originally. Materials remain 82 cached
texture sets, 110 definitions and 210.2 MB. There are 36 layout buildings.
No ashlar dependency source or budget constant changed.

Two `just content` runs were needed: the full second pass (7.2 s after
compilation), then a cached refresh after separating the foreground bench,
terminal and clinic sign placements (3.9 s). Evidence is in
`city/census-after.txt`, `city/stats-after.ron` and `city/bake-after.txt`;
first-pass evidence is preserved as `*-pass1.*`.

For the next captures: check continuous housing mass on the middle slope,
lower doorways and upper entrances at bank houses, and a broken green/rock
edge instead of exposed grey banks. At the street spawn, the lamps, terminal,
bollards, beds and paving edges should be visible immediately, with lanterns
above the nearest market aisle. At night, check illumination on the paving,
readable warm stall fronts and the clinic's cool side light. Look especially
for foliage swallowing entrances or the split-level decks cutting coarse
terrain; those require the renderer to judge.


Validation passed: `cargo fmt --all`, fatal-warning clippy on `content`,
`veyra-settlements`, `veyra-buildings` and `strata-worldgen`,
`cargo test -p content -p veyra-settlements -p strata-worldgen`, the fitting
mesh/LOD tests, and `just ci` (workspace checks plus the isolated server
and headless dependency checks and tests). The new gates retain 650 to 800
dwellings, at least twenty split-level homes, nearby arrival lighting, and
both split-level doorway approaches on their respective benches. Existing
spawn/collider, coplanarity, ground-clearance and census gates remain green.
Checks used `CC=clang HOST_CC=clang CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2`.
No commit was made.
