# 0005: Rivers, places and the roads between them

Status: agreed 2026-10-04. Step 1 (the river network) built and reviewed
2026-10-04; step 2 (the carve) and step 3 (water near) built, reviewed and
their findings fixed the same day; step 3b (the rivers' shape) built,
reviewed and its findings fixed 2026-10-05; step 4 (procedural sites and
their buildings) built, reviewed and its findings fixed 2026-10-05. Their
records, "Step 1, as built" to "Step 4, as built", are at the end. None
committed, `just ci` not yet run on steps 2 to 4, and the ledger's new
baseline not taken. Step 5 (the road network) and step 6 (the road's
grade) built, reviewed and their findings fixed 2026-10-05; their record,
"Steps 5 and 6, as built", is at the end, not committed and `just ci` not
run. Step 7 (the crossings) built, reviewed and its findings fixed
2026-10-05 (the review's red patch was the cloud shadow, fixed in
`strata-atmosphere`); its record, "Step 7, as built", is at the end, not
committed and `just ci` not run. Step 8a (rivers and roads painted at
distance) built, reviewed and its findings fixed 2026-10-10; its record,
"Step 8a, as built", is at the end, not committed and `just ci` not run.
The rest of step 8 (falls, wet banks and reeds, markers and lamps) not
started.

## What is wanted

The owner, 2026-10-04: "rivers and terrain features with roads... small
bridges, cutting through hillsides etc. kind of oblivion style", then "we
need to eventually make a system for procedurally place settlements and
POIs, so maybe we can make a preliminary version of that", then, to the
plan below, "sounds good, place some structures to illustrate".

A walk out of Port Serein should follow a road that winds up a valley
beside a river, crosses it on a small bridge, cuts through a hillside, and
arrives somewhere: a place with a building and a name. Paths leave the road
for things worth finding.

## What exists

- **Rivers:** designed in ticket 0004 (steps 6 to 12) and ADR 0010, not
  built. Water is one plane at sea level. The character swims against it.
- **Roads:** one 1.2 km road out of Port Serein
  (`strata_worldgen::settlement::Road`), traced greedily in 25 m steps
  over the natural ground, laid with an 8 m shoulder and the settlement's
  gravel. It cannot cut, fill or cross water, and it ends nowhere.
- **Places:** Port Serein, authored (ADR 0009). No other.
- **Buildings:** ashlar and `veyra-buildings` kits, baked by `just
  content`, with colliders the server builds from the baked files.

## Decided (2026-10-04)

1. Rivers first, then places, then roads: a road has to know where the
   water is to choose its crossings.
2. Procedural sites, a preliminary version, as ADR 0012: kinds as registry
   data, placed by the bake after the rivers, authored settlements fixed.
3. A site carries one or two existing buildings and a sign on a level pad,
   so a road visibly arrives somewhere.
4. Density to start from: towns 8 to 15 km apart, hamlets and stations
   every 2 to 4 km, landmarks about 1 km off a road. To be tuned on the
   map.
5. Three road classes: a paved service road (wide, gentle, bridges and
   cuttings), a gravel track, a foot trail that follows the ground and
   fords streams.
6. Bridges: local stone piers and abutments, a prefabricated deck and
   rails.
7. The look is tuned on one stretch: Port Serein's road carried inland to
   its first crossing and its first cutting. The bakes are world-wide.
8. A sample may cost 1.9 microseconds in the mountains (from 1.75).
9. The world moves again at the carve and at the road grade; the ledger
   takes a new baseline after each.

## Steps

Each ends on a `just worldmap` layer or captures a person looks at, and on
`just ci`.

1. **The river network** (ticket 0004 step 6, ADR 0010). The bake per land
   mass, lazy and hashed into `world_version`; a `-rivers` worldmap layer
   with rivers by discharge, lakes and falls. No change to the ground.
   Measures: the bake's time for the masses near the spawn and for the
   largest found; rivers per 1 000 km2; the longest; lake share of land;
   falls and their heights; what comes within 3 km of Port Serein.
2. **The carve** (ticket 0004 step 7). Valleys by discharge and province,
   channels, gorge walls on the strata, lake beds; `Sample` carries the
   water record. Colliders, placement and the fog follow because they read
   the sample. Everything pinned is blessed once; fixtures re-sited where a
   river runs through them; the ledger's baseline.
3. **Water, near** (ticket 0004 step 9). A water surface per terrain region
   from the mesher's grid at the river's and the lake's own level, the sea's
   shading with flow; wading slows, a swimmer is carried, below the render
   layer for the server. First point to walk and look.
4. **Sites** (ADR 0012). The registry `assets/sites/`, the placement in the
   bake, the `-sites` worldmap layer with names, the pad in the sample, and
   the buildings: a first set of kinds (town site, hamlet, monitoring
   station, landing, pass shelter, landmark) each with a small baked group
   from the existing kits and a sign.
5. **The road network.** Routes between sites by class, crossings recorded,
   Port Serein's road as its first link; a `-roads` worldmap layer. No
   change to the ground beyond what the old road already did.
6. **The road's grade.** A profile with a greatest gradient per class; the
   sampler cuts the ground down to it with steep banks in the province's
   rock and fills up to it on embankments; the carriageway's material by
   class; nothing grows on it or its verge. Trails follow the ground and
   only clear and wear it.
7. **Crossings.** A bridge kit in `veyra-buildings` sized by span (piers,
   abutments, deck, rails), baked per span class, placed from the network
   with colliders; fords for trails; culverts where a road crosses a creek.
   The showcase stretch is captured here.
8. **Far and dressing.** Rivers and roads painted by the terrain's shader
   at distance (ticket 0004 step 10); falls (step 11); wet banks and reeds,
   markers and lamps by the road (step 12).

Ticket 0004's step 8 (the coast) is not part of this ticket.

## Risks

- **The bake's size.** The largest land mass found is over 1 000 km across:
  at 512 m that is millions of nodes. The cap by forced divides has to hold
  the bake to seconds, off the frame, on a machine that has frozen under
  load: the bake runs on a capped pool.
- **Port Serein.** A river that wants to run through the city's extent, or
  a road grade that changes the ground at its exit, would move the
  fingerprint its buildings are fitted to. The network routes round the
  extent and the grade starts outside it.
- **The sample's cost.** The carve and the grade each add work at every
  point near a line. The bucket grid keeps points far from any line at one
  lookup; the bound is 1.9 microseconds and is measured at each step.
- **Short rivers.** The land is narrow, so a road beside a river for
  kilometres may be rare. Step 1's measures say how rare before anything is
  carved.
- **Sites that move.** Until settlements are generated the cost of a site
  moving is nothing; once residents and missions name sites it is a world
  reset. ADR 0012 records it.

## Not in this ticket

Generated towns on the sites, residents, missions, signs that say anything
true about distance, traffic, and long lowland rivers.

## Step 1, as built (2026-10-04)

The river network of ADR 0010, decisions 1, 2, 4 and 6, and the query of
decision 3, with a `-rivers` worldmap layer. **The ground did not move**:
`ProceduralWorld::sample` returns the same bits (`snapshots::seed_42_samples`
passes unchanged, no snapshot file moved) and `GENERATOR_VERSION` stays 11.
The one pinned value that moved is `world_version`. The review's four
"should" findings were fixed (the version's reach, the constants it missed,
the ADR's record, the falls along every coast) and most of its nits; this
record is the build after them.

### What was built

`strata_worldgen::hydrology` (no Bevy, no new dependency), in
`hydrology/{mod,field,mass,route,rivers,index}.rs`, with
`ProceduralWorld::coarse_ground`, `hydrology()` and `water_at` in
`procedural.rs` and `Site::extent_distance_at` in `settlement.rs`.

- **The field.** A lattice of 512 m, computed in tiles of 128 nodes on a pool
  of at most 16 threads the first time anything asks and kept. A node is the
  continent's base and the range's own metres; every biome relief octave at
  least three lattice spacings long (the first always) by the biome blend,
  with the mountains' rise, a range's crest growth and the foothills' flank
  damping as the sample applies them; a valley noise; the climate's moisture
  as the rain; the dominant province's `keep_pits_m` and `sink`. Below 150 m
  of base a node is sea with nothing else computed.
- **The mass.** Land nodes 8-connected, found by flood fill from any of them,
  named by the lowest node. Over 2^20 nodes it is cut into its parts inside
  256 km cells, each border a forced divide; a part with no coast drains
  over its cut edge. Whether a mass is over the cap is decided by counting
  it, so the parts are the same whoever asks first.
- **The routing.** A priority flood from the coast keyed on level and then
  node coordinate fills every pit; a pit at least its province's
  `keep_pits_m` deep is a lake at its spill level and drains through its
  lowest outlet (passing over any candidate whose water would come back into
  the lake); the rest are breached by a second flood. Rain less the
  province's `sink` is summed downstream.
- **The rivers.** Chains of at least 0.1 m3/s, cut at confluences into
  reaches with a Strahler order; jittered off the lattice, corner-cut three
  times and swayed sideways, the ends kept so the network stays joined.
  Width `4.5 Q^0.5`, depth `0.4 Q^0.375`, speed the rest. The surface is the
  breached ground's, set again from the mouth upstream after smoothing so it
  never rises downstream; a node step dropping 20 m or more at a gradient
  over 0.3 becomes equal falls of at most 120 m, and the stretch below a lip
  runs at its foot's level.
- **Port Serein.** Nodes within 450 m of its extent carry no water and are not
  routed through, no mouth opens into that margin, and the jitter and sway
  fade in over the next 700 m.
- **The query.** A bucket grid over the reaches' segments answers
  `Network::water_at` with the ADR's record (surface, bed, depth, width,
  signed distance, flow, speed, discharge, order, kind); lakes from the four
  nodes round the point, the deepest lake there winning.
  `Hydrology::water_at` finds the masses under a point and bakes any not
  yet baked; `bake_area` bakes a square's masses up front.
- **The version.** `world_version` hashes `RiverParams`, the constants a
  bake depends on (lattice, deep sea, cap and cell, the relief octaves'
  floor, the noises' octaves, gain and lacunarity) and the content hashes of
  three canary networks on the canary world, baked on one thread: a whole
  island at node `[150, 54]` (77 km east of the origin; 5 310 nodes, 91
  reaches, 19 lakes, falls), the parts of it round `[150, 54]` and
  `[175, 41]` cut as if the cap were 1 000 nodes in 16-node cells (one
  reaches the sea, one only its cut edges), and a window of 23 by 21 nodes
  round Port Serein whose edge counts as coast. Together they reach the
  flood fill, the sea and edge outlets, the cut, the lakes kept and breached,
  falls, mouths and the settlement's margin. `GENERATOR_VERSION`'s doc now
  says to bump it for any change that moves the network, since the canaries
  see only their own ground.
- **The map.** `just worldmap` writes `-rivers`: rivers by discharge over
  the hillshade, lakes filled, falls ringed by their drop, a cut mass's cells
  and Port Serein's extent outlined. `--no-rivers` leaves it out.

### Where it left the design, and why

- **The rain is the climate's moisture, not `rainfall()`.** The bare march
  credits a range's rain to its windward foot and reads zero on every crest
  and lee slope, so the ranges shed no rivers at all. The moisture is the
  same rain blended with the noise share the biomes already see. The range
  south of the city still sits mostly in a rain shadow.
- **More than the lowest relief octave, and a valley noise.** With one octave
  a range's flank is a smooth bell on a 512 m lattice and its water ran down
  it in parallel threads that never gathered. The field keeps every octave
  at least 1.5 km long (two for the mountains, 6 and 3 km) and adds 6 m plus
  12 % of a range's own metres of fbm at 3 km. The carve has to cut the
  valleys this assumes.
- **The cap is cells, not a coarser grid.** A mass over the cap is cut into
  parts inside 256 km cells, each routed on the same lattice. The 92 000 km2
  mass under Port Serein cut into 64 km cells keeps 81.3 % of its river
  length as it was (within 50 m and 10 % of discharge); 128 km cells, 88.0 %.
- **The version is canaries, not the whole network.** Hashing every mass at
  a handshake would cost every bake. The review found the first canary (a
  window inland of the city) reached none of the flood fill, the lakes, the
  cut or the city's margin, so it became the three above, and the constants
  the review listed were added to the hash.
- **A mouth's last step is never a fall.** It runs from the last land node to
  a coast interpolated across the sea bed's slope, so its drop measured the
  sea floor; read as falls, those steps were 1 476 of 4 017 falls and ringed
  every coast. The carve grades that stretch to the sample's shore.
- **Province rules:** `keep_pits_m` and `sink` are used; `section`, `hang` and
  `along_grain` are the carve's.

### The measures (seed 42, release, 16 threads)

- **Port Serein's mass** (key `[-1017, 282]`): 351 234 nodes (92 074 km2), not
  capped, baked in 0.42 s from 63 field tiles: 7 355 reaches, 397 lakes,
  1 442 falls. Its index holds 179 255 segments and 2.22 M bucket entries.
- **Every mass within 100 km of the city:** 21 masses, 147 800 km2 of land,
  baked in 0.64 s in all.
- **The largest mass met** (a 2 km scan of 3 000 km round the origin, then
  baked at 512 m): key `[-803, 4214]`, 602 169 nodes (about 158 000 km2, a
  thousand kilometres across), 0.77 to 0.81 s from 117 tiles, 12 451 reaches,
  668 lakes. The cap at 2^20 nodes never fired on real ground. Memory
  high-water in the cap measure, 129 MB.
- **Density:** 78.2 reaches and 37.8 mouths per 1 000 km2; 170 km of river
  per 1 000 km2.
- **The longest:** 48.5 km from source to sea (source about (-403 357,
  318 539)). The largest discharge: 8.20 m3/s, 12.9 m wide, at about
  (226 456, -295 756).
- **Lakes:** 671, 1.89 % of the land.
- **Falls:** 2 431 (16 per 1 000 km2), one of them into the sea; tallest
  120 m (the cascade's cap), median drop 98 m, 90th percentile 115 m. Most are
  pieces of cascades down a range's flank; before the mouth fix there were
  4 017, 1 476 into the sea.
- **What comes within 3 km of Port Serein's extent:** three creeks, 1.9 m
  wide at 2.51 km, 1.5 m at 2.53 km (its mouth) and 1.6 m at 2.90 km. The
  first river is at 3.14 km, order 2: it runs 3.5 km almost level (103 to
  100 m, 6.0 m wide) from about (2 605, -6 135), an upland valley 5 to 6 km
  south-east of the city; falls 71 m over its next 1.4 km (6.5 m wide) to a
  confluence with a creek at about (-1 516, -3 977), surface 29 m; and
  reaches the sea 200 m on at (-1 721, -3 975), 2.32 m3/s and 6.9 m wide,
  on the coast south of the city. Within 10 km: 27 reaches, the widest
  8.5 m (3.58 m3/s at 6.62 km, near (6 253, -2 591)), the rest 1.4 to 3.2 m
  creeks.
- **The query,** one thread, a million random points over 40 km round the
  city: 48 to 50 ns a point on the network, 65 to 72 ns through the world
  with the masses found; 8.4 % of points within the 600 m reach.
- **A field node** costs 866 ns against 2 541 ns for a full sample (92 509
  land points).
- **`world_version`** costs 54 ms in a release build, 23 ms of it the three
  canaries on one thread.

### Tests

In `hydrology/tests.rs` (each `cargo test -p strata-worldgen --lib
hydrology -- --test-threads=4`, 11 passing):

- a synthetic island drains to the sea downhill; a deep pit is kept and a
  shallow one breached; discharge is the rain upstream;
- the canaries are pinned and the same on one and four threads and on a
  rebake, and reach what they are there for (lakes, falls, a sea end, a cut
  part with an edge end and none at sea, rivers round the city);
- the canaries run downhill to the sea or their edge: no rising surface, no
  lost water, no loop, no sink, no edge end on a whole mass;
- the canary island routes without loops and keeps its water: every node in
  the order once after its receiver, surfaces never rising, no lake outlet
  draining back, rain in equal to water out;
- no river comes within half its width of Port Serein's extent;
- the query agrees with the river lines (distance, level, discharge, width,
  flow, side) and finds every lake node wet;
- a cut mass is the same part from any of its nodes, and a node the count
  marked as over the cap bakes, without a count, to the part a fresh cache
  counts to;
- four threads asking at once bake each mass once; width, depth and speed
  multiply to the discharge.

The review's `a_bake_leaves_the_sample_alone` was removed: no sample reads
the network, so it could not fail; `snapshots::seed_42_samples` is the
guard. Ignored probes, run in release: `probe_masses`, `probe_cap`,
`probe_node_cost`, `probe_version_cost`, and `tests/rivers_measure.rs` for
the measures above.

### What was pinned

- `world_version`: `0x6b2e_68de_cf70_2047` on HEAD, now
  `0x741c_60ae_d128_7060` (`tests/version.rs`). A client built before this
  is refused.
- The canaries: `[0xa2ee_52e7_07b0_9abe, 0x7c8a_7817_cb0d_95fc,
  0x95f1_4f81_7b82_10c9, 0x26f7_9d74_dbc2_8398]`
  (`hydrology::tests::PINNED_CANARIES`).
- Nothing else: `GENERATOR_VERSION` 11, the sample snapshots, the drama and
  fog pins.

### Captures

`scratch/review/rivers/ps60-rivers.png` (60 km round the city),
`ps200-rivers.png` (200 km) and `near12-rivers.png` (12 km), with the other
layers beside them: `just worldmap --size-km 60 --center-x-km=-0.4
--center-z-km=-0.5 --no-view --threads 16`.

### What is left

- **Structures.** None placed: "place some structures to illustrate" is step 4
  (sites).
- **Falls.** 2 431, median 98 m, are many; `fall_gradient` (0.3),
  `fall_drop_m` (20) and `fall_max_m` (120) are for the owner to tune on
  the maps.
- **For the carve (step 2):** grade each mouth's last stretch to the sample's
  own shore (a mouth lies on the coarse coast, up to a node off the real
  one); cut the valleys the field's valley noise assumes; read `section`,
  `hang` and `along_grain`; the lake record's shore distance is an
  approximation from the interpolated slope, and a point whose four nodes
  are all sea gets no water even beside a mouth. From the carve the network
  is part of the ground, and every pinned sample moves.
- **Startup bake.** The game and the server do not bake at startup yet. A
  first `water_at` bakes on the calling thread with up to 16 field threads,
  which would oversubscribe from inside a Bevy task pool; give the cache one
  thread or bake before the pools start.
- **Memory.** Field tiles are never evicted.
- **Short rivers.** The land is narrow: a river beside a road for kilometres
  will be rare. Port Serein's nearest river a road can reach is the order-2
  one above: a road carried 3 km south from the city meets it near its
  mouth, or 5 to 6 km south-east on its level upland stretch, the likelier
  first crossing for step 7's showcase.

## Step 2, as built (2026-10-04)

The carve of ADR 0010's decision 3 and ticket 0004's step 7: every sample
ends by cutting river valleys, channels and lake beds into the ground, and
carries the water at its point. **The world moved**, and everything pinned
was blessed once, after the review's findings were fixed (below). This
record is the build after the review.

### What was built

- **The surfaces onto the ground** (`rivers::fit_surfaces`). Step 1's
  surfaces were the coarse field's: within 40 km of the city a fifth of the
  land vertices stood over a metre above the sample's ground (the worst
  61 m) and the 95th percentile lay 107 m under it. Each vertex's level is
  now the natural ground less the freeboard (0.3 m) and an incision of 3 m
  plus 8 m per root of discharge, plus 12 % of a range's own metres in a
  range (faded in from 0.1 to 2 m3/s), grown in over 500 m from a source;
  breached from the source down so it never rises, with lake levels, the
  sea and confluences as floors.
- **Breaches graded** (`grade_breaches`). Where something rose in a
  river's way the breach held the surface dead level, in a slot, for up to
  3.7 km, then dropped down the far side in a ramp. The surface now falls
  on a straight line from where it was first held to a vertex past the
  breach: the steepest line down that stays under every surface between
  and lowers none by more than half the breach's depth (5 to 30 m), looked
  for over the reaches below as far as 6 km, so a breach through a
  confluence is graded as one; a tributary whose confluence came down with
  it steepens into it at 1 in 20.
- **Mouths** (`grade_mouths`): the sample's own shore along or beyond each
  mouth, the reach run on straight to it up to 600 m, a vertex at the shore
  itself at sea level, and the last 400 m graded to it, never over the
  ground less the freeboard.
- **The routing reads the grain** (`route.rs`): where a province has an
  `along_grain`, a step across the fold axis is entered only once the
  flood has risen 30 m times the grain times the step's share across the
  axis squared past it, unless the node was reached along the grain first.
  Provinces without a grain route exactly as before.
- **The query** (`index.rs`): a point reads one cell; its runs of segments
  are scanned once, keeping those the blend may use; a point farther from
  every segment than any of their valleys can reach (the widest half-width
  their discharge and cut allow in any province, per bucket) is not carved
  and stops there. The blend weights segments by the eighth power of
  distance, cut off from one and a half times the nearest's distance to
  twice; the surface is a soft minimum of the segments' readings, scaled by
  their spread, so a steep tributary does not lift its main stem's bed at
  their confluence and a steep reach's bend stands no cliff.
- **The carve** (`carve.rs`, with `dual.rs`): a lake's bed levelled to the
  eased coarse ground with a ring at the level, blended out over 80 m plus
  4 m per metre the ground stands off the level; a flat-bottomed channel
  whose banks rise to the freeboard, filled up to it within a band (never
  over the sea or in a lake); a valley that only cuts, half-width (60 +
  110 sqrt Q) m by the province's section width, its `hang` for low orders
  and a narrowing across the grain, up to 440 m, wider where the cut is
  deeper than the section's steepest wall allows, its walls rounded into
  the ground above. Every number is carried with its gradient, so the
  sample's gradient is the carved height's. A cut over 150 m fades out by
  250 m. The bands a cut wall shows are the beds at its own height.
- **`Sample.water`**, `Option<SampleWater>`, 32 bytes; `Sample::in_water`.
  Under water cover is 0 and hydration 1; the bed and the wet bank are
  gravel (channel, fall) or sand (lake, mouth). `strata_flora::fit_at`
  plants nothing in water or within a species' `sea_clearance_m` over a
  water surface.
- **The startup bake.** `strata_sim::prepare_water_before_start` bakes
  within 128 km (`PrepareWater`) of the spawn and every settlement before
  the game or the server builds its app, so no task pool is running; the
  loader's world shares the bake (one network cache per process for the
  same seed, parameters and registries). The loader then bakes round an
  observer in the background on two threads once it comes within a quarter
  of the radius of every square baked so far. A field tile is computed
  once under a lock (two sampling threads each computed it). A test app
  without `PrepareWater` bakes 12 km round each settlement and nothing
  ahead.
- **Port Serein** is placed, and its road traced, over the uncarved ground
  (`sample_before_carve`), and the network routes round its extent; its
  fingerprint 6857689028393273736 holds.

### Where it left the design, and why

- **The routing prefers the grain.** ADR 0010 left `along_grain` to the
  carve. Rivers routed on the coarse field run off the ranges across the
  fold axis, and their valleys' walls took the folded belt's grain from
  2.33 to 1.71, under its gate of 2.0; narrowing those valleys made it
  worse (a narrower wall is steeper and the measure sums slope squared).
  Routed along the grain, the belt reads 2.31.
- **Surfaces graded, not held level.** The review found the breach's
  flat canals; the line costs depth (the cut's 95th percentile 44 m, 31 as
  first built).
- **A soft minimum, not the mean, for the surface.** The mean lifted a
  main stem's bed by up to 0.40 m where a steep tributary met it; a soft
  minimum at a fixed scale stood cliffs (50 in 1) where a steep reach's
  segments read tens of metres apart round a bend, so its scale is the
  readings' spread. Up to 0.07 m is left at a confluence (the blend of a
  shallow tributary's depth into its main stem's).
- **Deeper incision.** At 2 m plus 4 m per root of discharge a lowland
  valley was 4 m deep and read on no map; at 3 plus 8 it is 9 m at the
  median vertex and reads on the 12 km hillshade.
- **Cuts fade out from 150 m.** The coarse field leaves out the provinces'
  shapes and the beds, and where they raise a spur hundreds of metres over
  it a creek routed across it was held at the level before it: 514 of
  45 273 vertices within 100 km are cut over 200 m, 99 over 800 m, the
  deepest 1 735 m (57 km south-west of the city), a slot whose walls stood
  at 9 in 1. Those creeks now run into the hillside and out again.
- **Section shapes are code constants**; no province field was added.

### The review's findings, and what became of them

- *Mouths stepped up to 7 m at the shore*: a vertex at the shore at sea
  level; 0 of 39 mouths within 25 km step more than 0.5 m in 0.25 m beyond
  the natural ground's step (9 before, the worst 3.3 m); the worst 0.13 m.
- *Platform `hypot` in the bake*: both now square roots.
- *Two gates failing*: the folded belt's grain 2.31 (gate 2.0) by the
  routing above; the fog's basins by moving the plain hollow (below).
- *The bed rose at confluences*: at most 0.07 m over the running minimum
  over 214.8 km of reach within 25 km (0.40 m before); the test now holds
  the running minimum, with 0.1 m.
- *Flat canals*: graded; 4 stretches over 300 m cut over 15 m are left
  within 25 km (41 before), the longest 2.06 km at (14 830, -11 157),
  where the breach runs on past the 6 km look-ahead through three reaches.
- *Mouth test never sampled the shore; across lines stopped at the bank*:
  the mouth test walks every 0.25 m through the shore; a new test crosses
  whole valleys every 10 cm out to 460 m.
- *The carried gradient was wrong at 3 % of carved points*: of 3 807
  carved points within 25 km, 22 (0.6 %) are off by over 0.05 plus 10 %
  beyond the natural sample's own error, all straddling the half of a
  slope change left at a segment's end. Most of the review's 116 were not
  the carve's: the natural sample's own gradient is wrong near some
  coasts, at points the carve does not touch (at (20 934, 7 787) it reports
  a slope of 0.008 where the ground falls 0.5 in 1). A new test holds the
  carve's to 1 %.
- *The river strip's cost was not held*: held now (below).
- *No bake as the player moves; a bake inside the compute pool; tiles
  computed twice*: the pre-start bake, the bake ahead and the tile lock.
- *Lowland valleys too timid*: the deeper incision.
- *The captures*: retaken with the final build (below).
- Nits: `cell_margin` counts a mouth's run-on (four cells, its comment
  corrected); a cut wall's bed is relabelled at its own height.

### The measures (seed 42, release)

- **The sample's cost**, pinned to one core: lowland 1.165 us, mountains
  1.824 us (the bound 1.9 holds); along the order-2 river's valley south of
  the city 2.275 to 2.293 us against 1.959 to 1.969 us for the same points
  before the carve, the carve 0.31 to 0.32 us (0.30 to 0.39 as first
  built). `tests/perf.rs` now holds the carve's own cost there under
  0.45 us; the strip's own ground is over 1.9 before any carve. A river
  valley in the mountains would cost about 2.1 us.
- **The bake:** the game's pre-start bake, 84 masses within 128 km of the
  spawn and the city, 1.15 s with the registries and the world built; the
  loader's then 0.02 s. Port Serein's mass with everything fitted, 0.60 s
  on 16 threads.
- **Cut** (vertex natural ground over surface, within 25 km): median 9.1 m,
  90th percentile 32.3, 95th 44.4, 99th 57.3, the deepest 65.0.
- **Drama:** the folded belt's grain 2.31 (2.33 before the carve, gate
  2.0); basalt 1.64 and sandstone 1.33 (2.36 and 1.78 before, not gated);
  karst hollows 2.01 a km2, badlands steep 39.1 %; walkable 97.5 %; slope
  classes 0-2 deg 16.8 %, 2-5 26.4, 5-10 20.8, 10-30 25.7, 30-50 7.7.
- **Fixtures:** woodland, grove, valley, coast, city-bay, city-terraces,
  the city, the road's end, the origin, the walk's 800 m and the flight's
  14 km: nothing in water, nothing moved by the carve. None re-sited.

### What was blessed

- `GENERATOR_VERSION` 11 to 12 (once for the step).
- `world_version` `0x741c_60ae_d128_7060` to `0xca97_4757_c41f_38ee`.
- The canaries to `[0x400f_2d09_0073_7faf, 0x9f1d_b497_a50b_1693,
  0x5be2_76b3_1ac2_7097, 0x0296_7f1a_a5d2_3805]`.
- `snapshots::seed_42_samples`: the eight old points unchanged but for the
  version and `water: None`; three water points added (the confluence's
  bed 15.65 m, a bank 4.6 m off its centreline, a lake bed at 255.90 m
  under a 262.40 m level).
- `drama::seed_42_measures` as above; the gates unchanged.
- The world without geology `0x0e9a_9b2b_6f6d_403c` to
  `0x5c92_2792_dee4_a8ed`.
- The fog: the bay tile `0xcf66_767e_236b_3900` to `0xc00d_f986_ea57_ba22`,
  its nodes `0xecc6_dbd7_fce4_bc9a` to `0x2ceb_b31b_396d_5e53`, between
  them `0x2cd4_2dde_cec3_f299` to `0x3739_f096_995f_3c0b`. A creek's valley
  drained the plain hollow at (11 500, -7 700) to 145 m, 15 m over the
  study hollow's, so it and the knoll between moved, by the new ignored
  `fog_places` search, to (12 150, -2 950), filled to 176 m (46 m over),
  and (9 929, -4 326). The plain's fog over its top is 2.05 m (bound 5);
  in a river's valley, which the 128 m nodes cannot resolve, 11.04 m, a
  known gap held under 13 m by `fog_in_a_valley_is_a_known_gap`.

### Tests

`tests/carve.rs` (11): continuity across and along a river, at a
confluence, through a mouth's shore, at a lake's shore and across whole
valleys; the bed against its running minimum within 12 km of the city; the
carried gradient against the heights'; banks over the surface; dry ground
to the bit; no water in Port Serein and its fingerprint; the startup bake.
`hydrology::dual`'s gradients against differences. `tests/carve_measure.rs`
holds the probes behind the measures above (ignored, release).
`strata-flora`'s `nothing_stands_in_water`. Run, one at a time: the
worldgen library and every worldgen test binary, `strata-weather`,
`strata-flora`, `strata-sim`, `strata-terrain`, `strata-physics`,
`strata-atmosphere`, `strata`, `strata-net-replicon`, `game`,
`worldmap`, `content`'s settlement test; `cargo fmt`, clippy with warnings
fatal and `just doc` clean. Not run: `just ci`, `just server-check`, the
benches.

### Captures

`scratch/review/rivers/carve-confluence.png`, `carve-upland.png`,
`carve-fall.png`, `carve-lake.png`, `carve-gorge.png` (flycam, `--fog 0
--time 0.45`, 1 300 frames) and `carve-walk.png` (the character at
(1 550, -5 590)); `carve2-near12-shade-before-after.png` (the 12 km round
the city, before the carve and after), `carve2-after-near12-*` and
`carve2-near12-*` (12 km south of the city). The gorge's walls are banded
at their own height; the lowland valleys read on the hillshade; the lake
is a dry basin until step 3 draws water; a channel's small steep banks are
painted with the province's rock bands, which shows as a dashed line down
a steep creek and as slivers where a karst slot's wall stands bare.

### What is left

- **The routing's ground.** The network routes on the coarse field, which
  leaves out the provinces' shapes and the beds: the hundreds of metres a
  spur rises over it are why the deepest cuts fade out, and why breaches
  exist at all. Routing on that ground, or settling each reach sideways
  onto the sample's low ground, is the fix; it moves every river.
- **Long breaches** that run on past the look-ahead stay level (2 km at
  (14 830, -11 157)).
- **The vertex kink**: half of a reach's change of slope is left at each
  segment's end.
- **The owner's bound** is 1.9 us in the mountains; a river valley there
  costs about 0.3 us over its ground.
- **The natural sample's gradient near some coasts** is wrong (not the
  carve's).
- **Step 3** (water, near) reads `Sample.water`: the surface is the soft
  minimum's, continuous across confluences; `in_water` is true only in a
  channel or under a lake's level; a creek whose cut fades out from 150 m
  has no water record where it is gone; mouths are at sea level at their
  shore vertex. The lake's record gives its shore distance from the eased
  coarse slope, an estimate.
- **Falls** keep step 1's positions and take the fitted drops; graded
  breaches leave fewer, gentler ones. Drawing them is step 8.

## Step 3, as built (2026-10-04)

Water drawn near and felt by the character, below the render layer for the
server. **The ground did not move**: `ProceduralWorld::sample` returns the
same heights, `GENERATOR_VERSION` stays 12, no pin was blessed
(`seed_42_samples` is unchanged: the one field the record gained is not
serialized). The review's must and its six shoulds were fixed or answered,
and most of its nits; this record is the build after them.

### What was built

- **The query** (`strata_worldgen`): `Sample::standing_water`, a lake's
  water, or a river's within its banks (`SampleWater::bank_m`, from
  `RiverParams::bank_top_m`, the one formula the carve's channel and the
  query share, held together by `carve::tests`); `in_water` stays as it
  was, a floodplain under its river's level counting. `SampleWater::cut_dm`
  is how far the carve lowered the ground at the point, decimetres, in the
  record's padding (it stays 32 bytes, asserted at compile time) and not
  serialized.
- **Felt** (`strata_sim::water`, `strata_physics::character`): the sea's
  one level is now `water_here`, the sea or the higher river or lake
  standing at the character's point, one sample a tick, taken only once
  there is ground under the character (whose colliders sampled the point's
  hydrology cell already, so the tick never waits on a bake), the same on
  a server. Walking slows with the depth over the feet to the swim's speed
  at `wade_depth` (0.85 m); deeper the character floats as in the sea; the
  flow carries a body by the share of its height in the water times
  `current_share` (0.8), never stored in its own speed. A fall holds
  nothing up (no float), carries a body from the waist up at least
  `fall_speed` (6 m/s) times the share, faster than a sprinting swim, so
  nobody swims or wades up one. The tunables are the controller's, beside
  `swim_factor`. The protocol was not touched.
- **Drawn** (`strata_terrain::river`, `assets/shaders/river_water.wgsl`):
  a surface per region from the mesher's own samples on its task, the
  terrain's triangles and diagonal, morphed by the terrain's band, on a
  child of the region's entity (culled, rebased, despawned with it) as a
  small Bevy mesh. Cells with a corner whose water stands are drawn; a
  floodplain's vertex lies under its ground; a cell holding two waters (a
  lake beside the river under it, a perched lake's open rim) is left
  undrawn; a lake's open water is merged into rectangles; a lake's vertex
  is level, and a neighbour further off than any water runs is no slope of
  this one's. A river narrower than a level resolves (its spacing at the
  finest level, two spacings above) is not drawn there, and one the parent
  level leaves out fades out over the morph's band. The shading is the
  sea's, moved to `water_common.wgsl` and shared: ripples by a flow map at
  the water's speed, the sea's chop scaled down on still water, foam on
  steep water and falls, flecks of it at a flowing bank, depth from the
  prepass. The water writes its depth, so the sea plane does not foam in a
  channel under sea level; at sea level it is discarded and the sea draws
  the mouth. Its mesh bytes count against the terrain's upload budget at
  24 times a store byte (`MESH_PATH_WEIGHT`, the mesh path's cost against
  the store's). `--no-rivers` builds none.
- **The carve's faults**: nothing is placed on ground steeper than 1.2
  (`strata_flora::CARVED_WALL_SLOPE`) that the carve lowered by over half
  a metre (`CARVED_WALL_CUT_M`); within a metre of the water by a channel
  or round a lake the ground takes the bed's gravel or sand and bares no
  rock, darkened and glossier by the water line; steep ground the carve cut
  (cut 0.5 to 6 m, slope 0.7 to 1.2, 2 to 8 m over its river) reads its
  rock bands at a warped height and at under half their contrast.

### Where it left the design, and why

- **One more field on the record.** The review found the cut-wall look
  and the placement rule keyed to the valley's record, which reaches 440 m
  from a river: every natural cliff in a valley lost its cliff kit, and the
  look ended in a hard seam where the record did. Nothing in the record
  said whether the carve had touched the point. The carve's own cut is
  continuous wherever its height is, so both rules now fade with it, and
  natural cliffs in a valley keep their kits and their ruled beds.
- **The look is the steep cut's.** Gated on the cut alone, a shallow
  valley's whole gentle flank (cut by 10 to 19 m at slope 0.28, at the
  fall's pose) read as wavy bands against the ruled benches above it; it is
  now only where the cut is steep too.
- **Creeks are near things.** A creek narrower than a level's spacing drew
  as a zigzag of cells with films of water on its banks; it is now drawn
  only where the ground resolves its channel and fades out before the
  level that does not. Far rivers stay step 8's.
- **Falls carry, they do not float.** With the current fixed, a sprinting
  swimmer (4.5 m/s) made 2 m/s up a fall's 2.4 m/s; a fall now carries 4.8
  m/s and lifts nothing, and its face stands at its own slope.
- **The river is bluer and less clear** (colour `[0.025, 0.08, 0.12]`,
  absorption 1.6 a metre, from `[0.045, 0.1, 0.105]` and 1.1): with its
  gravel showing through it read as asphalt from altitude.

### The review's findings, and what became of them

- *Must: the current leaked into the stored speed*, so a swimmer could not
  swim up any river. The blocked-axis guard now measures the body's own
  motion (the move less the current) and keeps no speed backwards from
  it; `a_swimmer_makes_way_upstream` swims up a 0.7 m/s channel at 4.6 to
  6.4 m in 3 s.
- *The cut-wall look switched on across the whole record and ended in a
  seam*: gated on the carve's cut and the slope (above). At the fall's
  pose the wavy region and its seam are gone; at the gorge the walls keep
  the look, fading out at their foot and their shoulder.
- *The placement rule bared every natural cliff in a valley*: gated on the
  cut; `nothing_clings_to_a_carved_wall` now holds a natural cliff in a
  valley to its kit's fit and a cut one to none.
- *Bevy meshes on the streaming path, unmeasured*: the bytes are counted
  against the upload budget at the mesh path's weight, the worst frame is
  logged, and the flight was measured (below). No bench was run, by this
  step's rules; the lead's flight fixture with and without `--no-rivers`
  is the measure that settles it.
- *A swimmer could climb a fall*: falls do not float and carry faster than
  the sprint's swim; `nobody_climbs_a_fall` sprints up a fall's face (1.5
  in 1) and up a cascade one can stand on (0.5 in 1), waist deep.
- *Lakes cut by an outlet's valley, falls*: the vertex slope was read
  across the lake's level and the river's far below, which foamed level
  lake water as steep; a lake's vertex is level now and the perched lake
  test holds it. The pale wedge that stays beside the lake in
  `water-lake-far.png` is ground, not water: the perched lake's dry sand
  bed, sloping down to the cut. The grey sheets on the hillsides in
  `water-lake.png` and the white lumps at the outlet are ground and rock
  pieces too: the carve's gravel bed on an outlet valley's floor, there
  in step 2's `carve-lake.png` before any water was drawn. The sawtooth
  ends and the fall's look are left (below).
- *Banks painted with a line, rivers like roads, gorges striped*: the
  bank's foam is flecks the noise alone picks, near a flowing bank only;
  creeks no level resolves are not drawn; the water is bluer. Rivers from
  altitude still read as straight strips of even width: that is the
  network's geometry and the carve's channel (steps 1 and 2), not the
  water's. The gorge's bands are the owner's call (below).
- Nits: the bank formula is shared with the carve; the river tests now
  separate a floodplain under the level from a bank over it; the sea's
  walking now slows from the first centimetre of water over the feet (it
  was full speed until the centre went under), which is recorded here;
  movement and placement rules are not in `world_version` (below); the
  invisible strips of water a cell wide where two waters meet are left.

### The measures (seed 42, release, xvfb, not a benchmark)

The `river water` log line, every two seconds, read at 73 to 83 s:

| pose | regions with water | vertices | KiB | worst frame | water a region meshed |
|---|---|---|---|---|---|
| confluence | 120 of 794 | 17 662 | 1 111 | 134 KiB, 4 regions | 27 of 12 256 us |
| upland | 126 of 788 | 18 355 | 1 149 | 117 KiB, 4 | 29 of 12 904 |
| upland bank | 114 of 785 | 16 951 | 1 063 | 108 KiB, 5 | 29 of 12 838 |
| lake | 211 of 785 | 42 131 | 2 692 | 183 KiB, 3 | 63 of 15 351 |
| lake far | 233 of 794 | 43 023 | 2 738 | 129 KiB, 5 | 71 of 14 855 |
| lake near | 234 of 794 | 42 532 | 2 699 | 146 KiB, 7 | 69 of 14 627 |
| fall | 107 of 779 | 9 534 | 586 | 71 KiB, 5 | 27 of 13 767 |
| gorge | 146 of 785 | 17 185 | 1 072 | 93 KiB, 4 | 34 of 13 664 |
| mouth | 119 of 797 | 17 704 | 1 112 | 92 KiB, 4 | 26 of 12 048 |
| walk | 115 of 791 | 16 767 | 1 054 | 91 KiB, 4 | 27 of 12 696 |

- **Creeks left to the levels that resolve them** took the upland pose
  from 29 438 vertices (209 regions) to 18 355 (126), the fall's from
  17 190 to 9 534. Merging a lake's open water had taken the lake's view
  from 403 107 to 46 373.
- **The mesh path's worst frame** is the first second's load of a pose,
  every finished region at once: at most 183 KiB, which the streaming
  record's 6.6 ms a megabyte puts at 1.2 ms on the render thread, against
  the 4 MiB budget's 16 store regions. The water's bytes now count 24
  times over against that budget, so a frame of 183 KiB of water takes
  the place of about 4.3 MiB of store writes.
- **The flight** (`--flight --flight-heading 1.5708 --spawn -2000,-6200`,
  3 000 frames, with and without `--no-rivers`): 189 of 14 813 regions
  built water, 970 KiB in all over 146 s (about 6 ms of mesh path in all
  at that rate), the worst frame 67.5 KiB at the start. The route is
  mostly over the sea after its first minute, so it is a weak test of
  river country; main 5.85 ms mean with water and 6.92 without, render
  4.44 and 9.72, both inside xvfb's paced noise. The lead's flight and
  route-walk fixtures with and without `--no-rivers` are the measure.
- **Physics**: wading 0.5 m deep covers 3.3 to 3.8 m in a second against
  4.5 to 5.3 on the bank; a swimmer drifts 2.4 to 3.2 m in 5 s in a 0.7
  m/s channel and makes 4.6 to 6.4 m upstream in 3 s at the swim's walk;
  on a fall's face a sprinting swimmer is carried over 2 m down in 2 s,
  and on a waist-deep cascade makes no way up.

### Tests

`strata-sim` `water` (2): a river stands only within its banks; a mouth
hands over to the sea. `strata-physics` `tests/headless.rs` (5 new): a
walker in a shallow channel is slowed by its depth; a swimmer drifts
downstream; a swimmer makes way upstream; nobody climbs a fall; past its
banks the river does nothing (a bank over the surface and a floodplain
under it); with the whole file (16) and the library. `strata-terrain`
library: the river module's 7 (a strip and nothing on dry ground, the
morph targets, the sea's mouth, the perched lake and its level water, open
water merged, a creek drawn where its level resolves it, neighbours'
shared edge) and `only_a_steep_cut_is_a_wall`; the shaders test and the
headless pipeline. `strata-worldgen` `the_bank_top_is_the_carves` and the
pinned samples. `strata-flora` `nothing_clings_to_a_carved_wall`,
`nothing_stands_in_water`, the species limits and the cell snapshot,
unchanged. Each binary on its own with four test threads; `cargo fmt`,
clippy with warnings fatal and `just doc` clean. Not run: `just ci`,
`just test`, `just server-check`, the benches.

### Captures

`scratch/review/rivers/water-{confluence,upland,upland-bank,lake,lake-far,
lake-near,fall,gorge,mouth,walk}.png` (flycam but the walk, `--fog 0
--time 0.45`, 1 500 frames, the poses of `water_shots.json` in the
session's scratch), and `water-flight-{1500,3000}.png`, the flight below.

### What is left

- **Perched lakes** (east of the lake at (22 340, -4 781)): the carve cut
  the lake's rim below its level next to its outlet's valley, so the water
  ends in a sawtooth at its last corner over a dry, sloping bed. The fix
  keeps the outlet's cut at or above the lake's level beside the lake, and
  moves the ground.
- **Rivers at distance** read as straight strips of even width, and creeks
  are drawn only near: painting rivers far is step 8; the straightness is
  the network's.
- **Gorge walls** read as wavy, zigzag bands at under half contrast rather
  than ruled lines; still banded. A non-oriented read on cut walls, or the
  beds' own colour bands, is the owner's look decision.
- **Falls** are a steep ramp foamed white, a zigzag where the ramp is
  narrower than the spacing; their sheet and spray are step 8.
- **The handshake** folds heights alone into `world_version`: a client and
  a server built either side of this step disagree on wading and on which
  valley walls hold rock pieces (so colliders) without being refused.
  Folding a movement-rules version in is the lead's call.
- **Invisible water**: where a cell is left undrawn as two waters or a
  perched rim, the physics still has water at its wet corners, a strip a
  cell wide (1 m at the finest level): a hitch, not a trap.
- **For step 4** (sites): `Sample::standing_water` says where water stands
  and `water_here` what a body feels; a ford is where a road meets a
  channel shallower than `wade_depth` (0.85 m) and slower than a walker
  makes way against (a walker at 5 m/s, the flow carrying 0.8 of the
  water's speed by the share of the body under), and a landing wants a
  lake's open water (merged rectangles) or a mouth, whose last stretch is
  the sea's to draw. Mouths sit at sea level at their shore vertex;
  confluences keep a continuous surface (step 2), and lake shores are the
  record's signed distance (negative inside), an estimate from the coarse
  slope.

## Step 3b, as built (2026-10-05)

The rivers' shape: water was drawn, and a river seen from 140 m up was a
strip of even width in long straight runs with sharp bends, a lake beside
its outlet's valley a wedge of water over a dry bed, and creeks ran through
spurs. **The world moved**; everything pinned was blessed once, after the
review's fixes (below). The review's three musts and four shoulds were
fixed or answered, and most of its nits; this record is the build after
them. Not committed, `just ci` not run, the ledger's baseline not taken.

### What was built

- **Settling** (`hydrology/shape.rs`, in the bake). A reach's segments over
  `resample_m` (100 m; the corner cutting leaves 64 to 90) are split; every
  junction moves once in two dimensions onto the lowest natural ground
  within 48 m worth the move, carried along its reaches over 150 m; every
  second vertex of a reach looks across its line within `settle_search_m`
  (160 m, coarse then fine) for the lowest ground worth `settle_slack`
  (0.03 m a metre), the offsets smoothed, limited to half a segment of turn
  and to two fifths of the way to any other reach, faded in from fixed ends
  and settlements, never into the sea or another lake. A tributary meeting
  square on is bent over its last 80 to 200 m to join at `confluence_tan`
  (40 degrees) off the main stem's upstream line, unless that would cross
  it. The sway is held down by a province's grain.
- **Meanders**, analytic: each vertex holds an amplitude (`meander_m`) and a
  phase; the carve reads a C2 periodic polynomial (`shape::sine`) of the
  blended phase, each bend sized by a hash of its index over a slower
  wander 4.7 bends long, every `bend_widths` (12, varied by 35 %) channel
  widths. The amplitude is `bend_amplitude_widths` (4.2) widths, at most
  30 m, times how flat the natural floor is either side (from the fine
  search), how gentle the surface (a fifth kept on steep reaches), how
  shallow the cut (15 % kept in a gorge cut 90 m), a noise, and capped by
  the line's turning radius, the room to other reaches, falls, a reach's
  ends (so every junction is on the line), mouths and settlements. Phases
  are set from the mouths up, so every reach at a junction ends at the
  phase the reach below starts at. `River::centreline` gives the channel
  every few metres with its width and depth; `FORD_DEPTH_M` (0.65) is what
  a road looks for.
- **Riffles and pools**: the width is `1 + riffle_share (0.3) x share x
  wave` over the mean (wide at a crossing, narrow at an apex, a mean of one
  over each bend; a straight reach keeps 35 % of it), the depth the mean's
  over that. The gravel reaches 2.5 bands further up the bank on a bend's
  inside (the bar). The bank's two "at least"s are softened by a quarter
  metre, in the carve and `bank_top_m` alike.
- **The carve** reads the channel's distance (each segment's line offset
  less the wave, by its size, over the wave's secant) for the channel and
  its banks, and the line's for the valley, whose floor widens by the
  amplitude and is level across the belt. Far from the channel no wave is
  read.
- **Lakes**: the bed is the fine ground's shape (the natural ground less
  its coarse part and its mean over the lake, `Lake::detail_m`) set on the
  coarse basin, softly held between two bounds off the coarse shore, so the
  shore wanders within 150 m inside it with the fine ground's bays and
  spits, and the rim outside rises at 1 in 25 from the level. Water stands
  only inside the coarse shore. Outside it, out to twice the blend's band,
  a shallow dip (under 6 to 12 m below the level) that the blend gives back
  under the level is raised to the rim's slope, so no dry hollow stands
  beside the water. The reach leaving a lake is held at its level until it
  leaves the rim (its gate, or 160 m past the coarse shore,
  `rivers::LAKE_HOLD_M`), where it gets a vertex of its own; neither the
  breach grading nor the taper into a lower confluence lowers that
  stretch. After the river's carve the rim is put back, through two soft
  ramps, wherever a valley lowered it under the level, but for the channel
  of a river at the level. A lake's distance inside its shore reads the
  eased coarse slope held off nought softly (`LakeHere::slope`).
- **Walls**: a wall cut over 15 to 45 m in a stepped or slot province takes
  its provinces' beds' benches and risers by their weights, faded on walls
  whose face (the wall's own slope, `wall_slope`, continuous) stands
  steeper than twice the natural beds' fade, the fade carried with its
  gradient.
- **The grain's narrowing** reads the segments' directions downstream
  averaged and not made a unit (`RiverHere::course`), eased to across the
  grain where reaches running different ways nearly cancel.
- **The mouth's run-on** turns by up to a radian when the last segment's
  line misses the sea.
- **The worldmap** draws each channel's centreline.

### The review's findings, and what became of them

- *Must: most lake outlets were dammed.* 18 of the 20 outlets within 60
  km ran under a land bridge at the level and 0.3 m for tens of metres to
  almost 200: the lake held partly because its outlet was blocked. Two
  causes, neither the rim's: the breach grading lowered the held stretch
  from the lake's own vertex (the ground past the rim rises over the level,
  so the held stretch read as a breach), and the taper into a lower
  confluence steepened a short outlet back into its lake (lakes 368 and
  369). Neither lowers the held stretch now, and the hold ends at a vertex
  set where the reach leaves the rim, so the drop starts there and not one
  segment on (holding the next vertex instead ran perched lake 368's
  outlet out on a 37 m embankment). All 20 outlets now carry their water
  past the rim; `every_outlet_within_60_km_carries_its_water_past_the_rim`
  walks each one's first 500 m.
- *Must: the stepped wall's offset stepped the height.* The fade read the
  cut's gradient, which jumps where a clamp in the half-width or the wall's
  slope switches. It reads the wall's own slope now, continuous, with its
  gradient. The review's 500 lines across deep cuts, every 2 cm: steps over
  0.12 m where the natural ground has none 19 (worst 6.3 m) in the review's
  build, 8 now (worst 0.16 m, risers of the beds at 6 to 8 in one on
  sandstone and basalt walls, by design), 2 at HEAD. The 2.2 m wall beside
  lake 368 in folded rock (no steps) was the grain's narrowing: the unit
  direction downstream turned through a right angle in centimetres where
  two reaches' directions nearly cancel, swinging the valley's half-width
  from 134 to 117 m in 8 cm. It now reads the averaged direction, eased to
  across the grain where it is short; that line's worst is 0.16 m.
- *Must: a continuity gate was loosened.* Restored to `assert_smooth`
  (twice the natural crease, no turn), and it passes. The crease was not
  the river's valley nor the rim: the lake's distance inside its shore is
  the depth over the eased coarse slope, floored at a hundredth with a
  hard `max`, and the eased ground is flat at every lattice node, so the
  slope crossed the floor in the lake's bed and creased the bed's bound
  there (a turn of 0.13 under 1.3 m of water). The floor is soft now; the
  line's worst crease went from 0.0027 to 0.00003 m. The rim's "nothing
  where the river raised the ground" was a hard `max` too and is a soft
  ramp now.
- *Should: the tests could not see the dams*: the new outlet test above;
  the lakes' test still counts river water at the level as no leak, which
  is right once the outlets are walked.
- *Should: dry rings beside the lakes.* The rim's blend gave back natural
  ground under the level outside the coarse shore; a shallow dip there is
  now raised to the rim's slope (above). Dry ground under the level within
  150 m of a lake's water, 8 m grid, 30 lakes within 60 km: 43 956 points
  to 29 408; what is left is mostly natural ground beyond the rim's reach
  (a karst floor south-east of the lake at (22 340, -4 781), dips 100 m off
  behind a strip over the level), which no sand marks. The ring and the
  sand squiggle in `shape-lake-far.png` and the detached curve in
  `shape-lake-near.png` are gone.
- *Should: what still reads as a canal, a road or a wedge.* The wedge (the
  outlet's trench past a grass bridge) was the dam and is gone: the lake
  runs into its outlet's notch. The rest is left (below, "What is left"):
  valley walls stand off a straight line, a steep creek runs nearly
  straight, a gorge's floor is flat, a mouth's last 400 m has no meanders.
  Wandering the valley's walls needs a blended per-vertex width (a phase
  blended between two unrelated reaches turns too fast to drive it), and
  the carve's cost (0.43 to 0.45 us against 0.45) and the folded gate
  (2.02 against 2.0) leave no room for it in this step.
- *Should: the lake captures were re-posed.* The old poses are captured
  again as `shape-lake-near-old.png` and `shape-lake-far-old.png`; the
  latter now shows the outlet's notch cut through the ridge at the level
  and its channel falling past the rim.
- Nits: the carve's stale `steps` doc, the removed wider search's
  leftovers, the settlements' fade written three times and the test's
  `let _ = dir` are gone. Not done: barbed junctions (16 of 269 arms
  within 60 km point upstream) are still not bent, since bending one
  through over 100 degrees in 80 to 200 m hooks it and the bend is checked
  against no ground; the lake bed's gradient still takes the natural
  ground's for the fine part's (the coarse part's gradient is not carried
  by the natural ground), an approximation the gradient test tolerates; the
  carve's cost and the folded gate (below).

### The measures (seed 42, release, 16 threads)

- **After the review** (the probe in `scratch/review/rivers/probe3b`, Port
  Serein's disc to 62 km, 8 threads): outlets within 60 km with ground
  over their water in the first 600 m, 0 of 20 (18 before); centreline
  points within 40 km with ground over 0.25 m above the water 176 (320),
  all but four in two 500 m cells of the deep tail 36 km west-south-west
  (the `DEEPEST_M` fade); the folded belt 2.02, basalt 1.94, walkable 97.4 %.
  The carve's own cost, six runs pinned to one core: 0.431 to 0.451 us, one
  over the 0.45 bound; mountains 1.74 to 1.75, lowland 1.10. The bake and
  the sinuosity were not measured again (the review moved no line but the
  outlets' held stretches).
- **The bake**, Port Serein's mass: 1.00 s (0.64 before): settling 0.35 s,
  meanders 0.07, surfaces 0.07. 293 724 segments (244 282), 3.51 M bucket
  entries (3.01 M).
- **The carve's own cost** on the river strip, pinned: 0.435 to 0.438 us
  (bound 0.45; 0.29 to 0.30 before); 0.50 us as first built with the wave
  read per segment. Mountains 1.73 us, lowland 1.10.
- **Sinuosity**, the channel over its line, reaches of 400 m or more within
  60 km, median (10th to 90th): lowland (under 0.5 %) 1.082 (1.044 to
  1.119), middle 1.084, steep (over 3 %) 1.019 (1.001 to 1.071); 1.000
  before. Over the chord, lowland 1.150 (1.032 before).
- **Cut**, vertices within 100 km: over 60 m 4.47 % (2 439 of 54 609; 4.86 %,
  2 202 of 45 273 before), over 200 m 1.03 % (562; 1.13 %, 514 before),
  deepest 1 625 m (1 735). Within 25 km: median 8.4, 95th 48.1, over 60 m
  0.26 %. Settling sideways does not reach the deep tail: those creeks are
  routed by the coarse field through mountains the fine ground has and it
  does not (the deepest runs 1.5 km under a massif 57 km south-west of the
  city), not round a spur; looking three times as far moved no measure.
  Routing on finer ground is still the fix.
- **Lakes within 60 km of the city**: 30; perched 28 before (a point of the
  water beside ground under its level 24 m off), 0 now; none dry.
- **Drama**: the folded belt's grain 2.02 (gate 2.0; 2.31 before, 1.98 with
  the sway as it was, which is why it is held down by the grain; 1.99 with
  the grain read off the averaged direction alone, which is why it eases
  to the unit's square along one reach); basalt 1.94 (1.64); walkable
  97.4 % (97.5).

### Tests

`tests/carve.rs`: `lowland_rivers_wind_and_steep_ones_run_nearly_straight`
(lowland median over 1.06, steep under 1.04, within 40 km),
`no_channel_crosses_itself_or_another_near_port_serein` (centrelines every
2 m within 12 km), `a_riffle_keeps_the_mean_width_and_a_ford_comes_every_400_m`
(the mean width within 3 %, a ford every 400 m on rivers under 8 m, the
record's width at the centre within 15 %), `every_lake_within_60_km_holds_its_water`,
`valleys_near_port_serein_are_cut_no_deeper_than_their_ground_allows`,
`every_outlet_within_60_km_carries_its_water_past_the_rim` (the review);
the along, bank and bed tests now follow the channel's centreline, and the
bed test holds the water's surface (a bed rises out of each pool); the
lake shore's continuity is held to the old bound. `hydrology::shape`'s wave
test; the canary window is
baked on four threads against the canary's one. `tests/shape_measure.rs`
holds the probes behind the measures (ignored, release).

After the review, each binary on its own with four test threads, in
release: the `strata-worldgen` library (80), `carve` (17), `version`,
`snapshots`, `drama`, `geology`, `continuity`, `determinism`, `coverage`,
`port_serein`, `rainshadow`; `perf` pinned to one core six times;
`strata-weather` (the fog's pins unchanged); `strata-flora`; the
`strata-terrain` library's river and wall tests; `strata-physics`
`headless` (16); `strata-sim`'s water tests. `cargo fmt`, clippy with
warnings fatal and `just doc` clean. Not run: `just ci`, `just test`,
`just server-check`, the benches; the pins were not checked again in a
debug build after the review.

### What was blessed

- `GENERATOR_VERSION` 12 to 13.
- `world_version` `0xca97_4757_c41f_38ee` to `0x54d5_aa11_5e0a_56ee`
  (`0xb186_ddb7_dc5b_0cc9` as first built, before the review).
- The canaries to `[0x3809_1d37_e010_9538, 0xb098_99e2_366c_6568,
  0x0d85_1ec1_feb0_1267, 0xdf87_f3bd_734c_e1a3]` (two of them moved with
  the outlets' holds after the review).
- `snapshots::seed_42_samples`: the river moved, so its channel and bank
  points moved with it to (-1 587.3, -3 972.5) and (-1 588.0, -3 977.0);
  (20 000, -5 000) now lies in a creek's valley, 38 m lower.
- `drama::seed_42_measures` as above; the gates unchanged.
- The world without geology to `0xff5f_9970_99a7_dd1a`
  (`0xfe74_ceaa_91a2_3562` before the review).
- The fog: the bay tile `0xe8cd_45e1_e9fa_d846`, its nodes
  `0x338a_cf71_6b6e_3af5`, between them `0x709e_9c11_5c68_c3fe`; the same
  in a debug build; no fog place moved. The review's fixes moved neither
  these nor `seed_42_samples`.
- Port Serein's fingerprint holds; no water in its extent.

### Captures

`scratch/review/rivers/shape-{upland,upland-bank,confluence,lake-near,lake-far,gorge,mouth}.png`
(the poses in the session's `shape_shots.json`: the lake poses look at the
lake's water south of its outlet) and `shape-{lake-near,lake-far}-old.png`
at step 3's `water-lake-near` and `water-lake-far` poses, all taken again
after the review (`fix_shots.json`); `shape-{before,after}-{near12,ps60}-rivers.png`
and `shape-near12-upland-before-after.png` are the first build's.

### What is left

- **The deep cuts** (above): routing on the fine ground's provinces.
- **Meanders are an offset about the line**, so no bend overhangs (no
  gooseneck) and sinuosity stays under about 1.2; the bends are regular in
  shape though not in size.
- **Steep creeks** keep a fifth of their meanders and still run nearly
  straight down an open slope in a V, which from above reads more like a
  track than a stream.
- **Cut walls** keep step 3's banded look; benches show on gorges in
  stepped rock, the rest are eased slopes.
- **The bake** is a second for Port Serein's mass; a third of it is the
  settling's samples.
- **Valleys read as cuttings**: the walls stand off the settled line at a
  steady half-width, so from above a winding channel runs down a straight
  trough with ruled walls; a gorge's floor is flat and even; a mouth's last
  stretch and a steep creek run nearly straight. Wandering the walls wants a
  per-vertex valley width blended like the amplitude, which costs the
  carve a blended sum it has no room for (below).
- **The carve's cost** is 0.43 to 0.45 us against its 0.45 bound, pinned to
  one core (one run in six over), and the folded belt's elongation 2.02
  against its 2.0 gate: the next change to the carve has to pay for itself
  or the lead moves a bound.
- **Perched lakes** (lake 368 at (-44 258, -35 675), level 250.55 over
  natural ground at 195): the coarse lake stands tens of metres over the fine
  ground; its bed and rim are built up to the level and its outlet falls
  40 m past the rim in one segment. Routing on the fine ground is the fix,
  as for the deep cuts.
- **Barbed junctions** (16 of 269 arms within 60 km come in pointing
  upstream) are not bent.
- **An outlet's notch** through ground over its lake's level (`shape-lake-near-old.png`,
  `shape-lake-far-old.png`) is a flat floor at the level, gravelled its
  whole width, over a ridge: it reads as a road cutting. A mouth's last
  segment (`shape-confluence.png`) still runs straight to the sea with a
  kink where the run-on turned and a white strip at its end.
- **For step 4**: a ford is where `River::centreline` gives a depth of
  `FORD_DEPTH_M` (0.65 m) or less, at least every 400 m on rivers under
  8 m wide near the city (riffles between bends); a crossing reads the
  centreline, which is within a half-width of the carved channel at all
  but 17 of 116 000 points within 40 km. A confluence keeps its channels
  on the line for three quarters of a wavelength either side (no meander
  at a reach's ends), with a continuous surface; tributaries mostly join at
  40 degrees off the upstream line, a few barbed. A lake's water stands
  inside the coarse shore, its shore wandering up to 150 m inside it with
  the fine ground's bays; outside, the ground rises from the level at 1 in
  25 over the rim (about 140 m), and its outlet runs at the level through a
  notch in the rim and falls past it, so a landing wants the open water
  and not the outlet's notch. A mouth's meanders die out over its last
  400 m, and it runs to a vertex at the sample's shore, turned up to a radian to find it; its last
  stretch is the sea's to draw.

## Step 4, as built (2026-10-05)

Procedural sites (ADR 0012, decisions 1 to 6): kinds as registry data, the
placement in the bake after the rivers, the pad in the sample, the `-sites`
worldmap layer with names, and a small baked group of buildings and a
board on every site, stood the same by the server and the client. Built in
two halves (placement, then structures), reviewed together; the review's
four shoulds were fixed and most of its six nits. **The world moved**:
everything pinned was blessed once more after the fixes (below). Not
committed, `just ci` not run, the ledger's baseline not taken.

### What was built

- **Site kinds** (`assets/sites/*.site.ron`, `strata_registry::site`),
  loaded, patched in three rounds, checked and frozen as the provinces are;
  a bad file is refused with its file and field. A kind gives hard ground
  limits (slope, height band, biome and province weights), weighted wants
  (each the best of a few terms, any of them required): a distance to the
  sea, a river, a lake, a confluence, a mouth, a ford, a fall, a gorge, a
  site of an earlier kind or an authored settlement, or a measure of
  `summit`, `view`, `pass` or `cove`; its footprint, blend band, crown and
  deepest cut; spacing from its own kind, other kinds and settlements; a
  count per 1 000 km2, rank, facing rule, road class, building group and
  naming rules. The six: town, hamlet, monitoring station, landing, pass
  shelter, landmark. With no kind files the networks and every sample are
  step 3b's to the bit.
- **Placement** (`hydrology/place.rs`), inside each mass's bake after its
  rivers, on the ground its own network carves: every node surveyed once;
  the features from the network and the natural shore; kinds in rank
  order, nodes scored and taken best first (ties by a seeded jitter, then
  the coordinate); each refined to the best of nine places 170 m apart and
  then of nine 57 m apart round the best of those, and its pad tested (no
  water, no channel within the band, no carved wall, no settlement ground,
  nothing below the sea, cut and slope within the kind's); refinements in
  fixed batches, so the sites are the same on any thread count; a budget
  per kind bounds the time. Each site has an id (kind, mass, node), a name
  unique on its mass and never an authored settlement's, a facing, a pad
  level and an arrival on the pad's edge towards the gentlest approach.
  The sites and the kind files are hashed into the network, so into
  `world_version`.
- **The pad** (`sites::Pad`): the ground level (crowned) inside the
  footprint, blended back over the band with continuous height and
  gradient, developed (nothing grows or is placed) out to a 6 m skirt that
  fades over 8 m. Only the yard is laid in gravel: all of it inside 0.45 of
  the radius, thinning out to nothing at the edge by a threshold that
  varies over a couple of metres (`sites::gravel_threshold`), so its edge
  breaks up; the rest of the pad is cleared ground in the land's own
  material. A sample reads pads from the lattice cell it reads for the
  carve.
- **The groups** (`veyra_settlements::sites`, `content::site_groups`):
  per kind two or three designs from the existing kits (houses, workshop,
  composite modules as huts and an office, the beacon cut to a 20 m mast,
  containers, street furniture, the study kit's outpost as a depot), each
  also mirrored, so 4 to 6 arrangements leaving different sides open; two
  new `veyra_buildings::port::Fitting`s, `Board` (a name board under a
  hood with a lamp, blank stripes for lettering) and `Marker` (a cairn with
  a banded post and a lamp). `just content` writes each layout as
  `generated/sites/veyra/<group>.ron` beside 18 building files.
- **Standing them** (`strata_worldgen::sites::place_group`), a pure
  function of the site, its road class and the layout: never an
  arrangement past the pad; of the rest, one that keeps the way in from
  the arrival clear, seeded by the site's id; each building 4 cm into the
  pad under its lowest corner; the board just inside the edge beside the
  arrival, facing down the road.
- **At run time**: the loader reads the groups (`strata_sim::sites::SiteGroups`)
  and keeps `NearSites`, every site within 5 km of an observer, remade on
  the task pool after 250 m of movement; the physics builds static bodies
  within 600 m (released past 800 m), server and client alike; the client
  draws within 4 km (dropped past 4.4 km) at the same pose; no skyline or
  impostor. The name is a HUD card (`strata_client::ui::place`) while the
  player is on the pad or its skirt: a board shared by every site of a
  kind cannot carry a baked name. Nothing was added to the replication
  protocol's registrations.
- **The handshake** carries a fifth version, `site_groups`
  (`strata_sim::sites::site_groups_version`): every kind, its layout's
  bytes and the bytes of every file the layout stands, FNV-1a, a missing
  one as missing. `world_version` says where the sites are; this says what
  stands on them, which the content step makes.
- **The worldmap's** `-sites` layer (`--no-sites` leaves it out): a symbol
  per kind, names in a small built-in font, a legend, Port Serein marked;
  at 20 m a pixel or finer each pad to scale with its facing and arrival.

### Where it left ADR 0012, and why

- *Decision 3, authored settlements as fixed points*: they enter as
  spacing (each kind's `settlements_m`, Port Serein's extent and a
  kilometre clear) and their names are reserved; no site is placed
  relative to a settlement's buildings.
- *Decision 5, a road to its edge*: not built; the site records its
  arrival for step 5. The pad is gravel only in its yard (below).
- *Decision 6, fitted by ashlar's own grounding*: each building stands on
  the pad's own height under its lowest corner, which the pad's crown
  makes a few centimetres different per corner; ashlar's grounding would
  have needed the pad's ground in the authoring crates. A group is 2 to 7
  pieces, not one or two buildings, so a site reads as a place. The sign
  carries no text (a HUD card names the place).
- *What a version covers*: `world_version` does not cover what the
  content step bakes, so the handshake gained `site_groups` (above).

### The review's findings, and what became of them

- *Should: the server-and-client test shared one bake.* Its two worlds
  shared the process's river cache, so one bake fed both. The client's
  world now takes a cache of its own (`ProceduralWorld::with_own_water`,
  asserted with `shares_water_with`) and bakes on three threads against
  the server's four; the lists and every building's pose are equal to the
  bit. The three site tests take 54 s in debug (two bakes of Port Serein's
  mass).
- *Should: no version covered what stands on the sites.* The handshake's
  `site_groups` version (above), refused and told in words like the other
  four; `the_site_groups_version_covers_layouts_and_their_files` and the
  adapter's refusal test hold it.
- *Should: a kind's syllables could hang the bake.* The registry refuses
  rules whose shortest root is over nine letters or whose longest is under
  four (`names`), and `name` takes the last root cut to nine letters after
  256 draws; no shipped rule comes near it. Tests on both.
- *Should: the places did not sit where their names say, and every pad
  read as a car park.* Four changes. A channel is cleared by its own
  river's greatest meander (`RiverParams::bend_amplitude`) and not the
  largest river's 30 m. A finer refinement (nine places 57 m apart round
  the best of the first nine) brings a site up to its water. The hamlet's,
  landing's and town's water wants were pulled in (ford within 130 m,
  river 90, sea 100 for a hamlet; sea 60 for a landing; river 150 and sea
  120 for a town). The pads were sized to their groups (town 75 to 50 m
  with a 45 m band, hamlet 40 to 34 with 30, landing's band 30 to 24,
  landmark 14 to 10) and only the yard is gravel, its edge broken up. Pad
  edge to the nearest wetted water, within 60 km, median and 90th
  percentile: a hamlet by a ford 78 and 102 m, by the sea 62 and 288, a
  landing 22 and 44, a town 122 and 310 (the review read 98 m for Beth's
  Ford's pad edge and 94 for Daran Landing's off the map; the first build
  was not probed). A ford hamlet still stands outside its whole band's
  clearance of the channel, about 85 m from a creek's line.
- *Nit: the band's water checked at 8 points.* The band is sampled at 0.6
  of its width in eight directions and at 0.95 in the eight between; a
  kind that faces the water may have its band reach into the sea (the
  blend makes a little made ground into the shallows), others may not.
  The pad test now samples the band at a quarter, half, three quarters and
  0.95 of its width.
- *Nit: a capped mass's parts place apart.* Documented in `place`'s module
  doc and `pad_at`; not fixed. No mass of seed 42 within 3 000 km reaches
  the cap.
- *Nit: a band narrower than the skirt.* Refused: `blend_m` at least
  `PAD_SKIRT_M + PAD_SKIRT_FADE_M` (14 m), constants now in the registry.
- *Nit: the client's sites leaked on leaving the world.* Its `clear`
  despawns them. The physics' bodies and `NearSites` are still not reset;
  nothing leaves `InWorld`.
- *Nit: not strictly best first within a batch.* Documented in the module
  doc; deterministic either way.
- *Nit: names unique only on a mass.* Authored settlements' names are
  reserved; uniqueness across masses and English-word roots are not
  addressed.

### The measures (seed 42, release, 16 threads unless said)

- **The bake**, Port Serein's mass (351 234 nodes, 92 074 km2): 1.50 s of
  which placement 0.48 s (1.00 s before sites; 1.42 and 0.42 as first
  built); 5 228 sites on it (4 657). Every mass within 128 km of the origin
  (42): 2.82 s with sites against 1.77 s without, placement 1.07 s, 9 336
  sites. The 80 masses within 200 km: 1.91 s of placement.
- **Sites within 12, 60 and 200 km**: town 0, 24, 177 (nearest Gireclant
  at (11 663, -8 135), 14.3 km); hamlet 10, 152, 1 056 (Wack's Hold at
  (-1 878, -3 754), 3.6 km, at the mouth south-west of the city); station
  6, 114, 973 (Krot Station at (-2 333, -1 423), 2.1 km); landing 2, 13,
  150 (Bees Landing at (-2 105, -5 177), 5.0 km); pass shelter 0, 18, 238
  (Craicog Saddle at (6 144, -15 815), 16.7 km, its pad at 2 569 m);
  landmark 3, 66, 793 (Cebur Rim, a gorge's, at (7 850, -5 632), 9.7 km).
- **Spacing** within 60 km, least and median to the nearest of the same
  kind: town 9 036 and 12 092 m, hamlet 2 569 and 3 381, station 3 511 and
  4 757, landing 4 770 and 12 856, pass shelter 4 072 and 6 264, landmark
  2 162 and 3 862; least to any other kind 1 012 to 1 502.
- **The sample**, one core: 1 408 points on 88 pads within 30 km 2.213 us
  against 2.190 us with no sites; 1 232 points a kilometre off any 2.194
  us both ways. `tests/perf.rs` on cores 4 and 6: lowland 1.12 us,
  mountains 1.75 (bound 1.9), the carve's own cost 444 to 471 ns over six
  runs, three over its 450 bound (step 3b: 431 to 451, one in six over);
  core 2 runs slower on this machine (1.18, 1.85, 474 to 477). No pad lies
  in the perf strip; the carve's bound stands where step 3b left it.
- **The groups**: 6, 6, 6, 6, 4 and 4 arrangements (town, hamlet, station,
  landing, pass shelter, landmark), reaching 39.4, 26.3, 20.5, 26.6, 14.1
  and 5.3 m; the content bake 3.5 s from its cache.

### Tests

New: the registry's site kinds (load, patch, every refusal with file and
field, the band and the syllables); the worldgen library's pad, frame,
name, placement, board and yard tests and the canaries with and without
kinds; `tests/sites.rs` (thread count, spacing and counts, unique names
none a settlement's, no pad or band in water or a channel nor pad below
the sea or near Port Serein, the pad level and step-free walked every 5
cm, no kinds gives the same world and Port Serein's fingerprint holds);
`tests/sites_measure.rs`, the probes behind the measures (ignored,
release); `version`'s `one_site_kind_changes_the_version`; flora's
`nothing_grows_on_a_site_pad`; `strata-sim`'s three site tests;
`strata-physics` `tests/sites.rs`; the game's drawing against the
physics; `veyra-settlements`, `content` `tests/sites.rs` and
`veyra-buildings`' fittings; the client's card; the adapter's refusal.

Run after the review, one binary at a time, four test threads: in release
the `strata-worldgen` library (92), `sites`, `version`, `snapshots`,
`drama`, `geology`, `carve`, `continuity`, `determinism`, `coverage`,
`port_serein`, `rainshadow`, `perf` pinned, and `strata-weather`; in
debug `strata-registry`, the fog's pins, `strata-flora` `placement`,
`strata-sim`'s site tests, `strata-physics` `sites`, `strata-net-replicon`,
the client's `place`, the game's `sites::`, `content` `sites` and
`veyra-settlements`' `sites`. `cargo fmt`, clippy with warnings fatal and
`just doc` clean; `cargo tree -p server` finds no authoring crate and no
`bevy_render`. Not run: `just ci`, `just test`, `just server-check`, the
benches, an end-to-end server and two clients, `strata-terrain` and
`strata-physics` `headless` after the review.

### What was blessed

- `GENERATOR_VERSION` 13 to 14 (once, in the first build).
- `world_version` `0x54d5_aa11_5e0a_56ee` to `0x7a48_c3e0_6fb9_5fb9`
  (`0xb4fb_cd64_a3a0_f989` as first built).
- The canaries to `[0x1b5e_adb4_4535_ddf3, 0xa3b1_022a_5a87_4259,
  0x635f_c3f7_d8f2_e483, 0xa786_f91e_15a8_7112]`; a world without site
  kinds still bakes step 3b's.
- `snapshots::seed_42_samples`: the version line only; no pinned point is
  on a pad.
- `drama::seed_42_measures`: basalt's flat share 27.5 to 27.4 %
  (sandstone's 28.8 % as first built is back at step 3b's 28.7).
- The fog's bay tile `0x8abc_ba8a_b3cf_b208`, its nodes
  `0x5cc1_062e_78db_bdcd`, between them `0x9465_4bdc_454c_7a8b`, release
  and debug alike: pads in the tile are its ground.
- Port Serein's fingerprint holds, and so does the world without geology.

### Captures

`scratch/review/sites/{ps12,ps60,ps200,near3}-sites.png` (the city's
square at 12, 60 and 200 km, and 3 km round Wack's Hold and Bees Landing);
`site-{town,hamlet,hamlet-ford,station,landing,pass-shelter,landmark,overview}.png`
under xvfb (`--flycam --lock-camera --fog 0 --time 0.45 --frames 1500`,
the poses in the session's `site_shots.json`); `site-walkup.png` (on the
pad's edge at Wack's Hold, the card up) and `site-walkup-board.png` (15 m
out on the way in). The first build's are in `scratch/review/sites/first-build/`.

### What is left

- **The HUD card** sits right of centre at 2560x1440 (`site-walkup.png`):
  the slotted library's centred anchor scales half the window by the UI
  scale; the prompt chip shares it. The fix is in `bevy-slotted`.
- **A ford hamlet** stands about 85 m from a creek's line, since the whole
  band keeps clear of the channel; a band shaped on its water side would
  bring it to the bank.
- **The pads are still discs**: cleared ground round an irregular gravel
  yard, the buildings at the back of the yard. A town's 4 to 7 pieces on a
  100 m clearing read small from 150 m (`site-town.png`).
- **The landmark's marker** is 3.6 m on a 20 m pad.
- **Names**: unique per mass only; roots that make English words ("Game
  Bay", "Moment Mouth"); many "Port X" and "X Mouth" towns at 200 km.
- **The city's own buildings** are not covered by any version (only the
  tree library is, and now the site groups): a client with another
  `generated/settlements` is not refused.
- **The bake** grew by half a second on Port Serein's mass; a persistent
  pool, sharing the survey with the river bake, or a coarser lattice for
  the sparse kinds would claw it back.
- **The carve's cost** is 444 to 471 ns against its 450 bound on this
  machine's faster cores; the lead moves the bound or the next carve
  change pays for itself.
- **For step 5**: a mass's sites are `Network::sites()` in placement order
  after `world.hydrology().bake_area(..)`; `ProceduralWorld::sites_near(x,
  z, r)` (nearest first, bakes on the calling thread if needed) and
  `site_at(x, z)` find them. A `ProceduralSite` has `id` (`SiteId { kind,
  mass, node }`, `key()` one u64 within a world version), `name`,
  `centre`, `level_m`, `radius_m`, `blend_m`, `crown`, `facing`,
  `arrival` (on the pad's edge, the gentlest of eight approaches from the
  ring at radius, band and 40 m that crosses no water: where a road
  should end), `by` and `cut_m`; `pad_height_at(x, z)` is the pad's
  surface. A kind's `traits.road` is its class (town service; hamlet,
  landing and station track; pass shelter and landmark trail) and `place_group` keeps a way in from the arrival to the
  yard `way_in_half_m(class)` wide (4, 3 and 1.5 m) clear of buildings; a
  road should arrive along the line from `arrival` to `centre`. Port
  Serein's road is `world.sites()[0].road`, `points` from the city's exit
  (the frame's (165, -460)) out 1.2 km, the last its far end (`--at
  road`): the network's first link starts there. A road may cross a
  site's band but must not change the pad (its level is `level_m`), and a
  ford is where `River::centreline` gives a depth of `FORD_DEPTH_M` or
  less (step 3b).

## Steps 5 and 6, as built (2026-10-05)

The road network (step 5) and the road's grade (step 6), ADR 0012's
decision 7: a backbone of service roads between the towns and Port
Serein's road, tracks to the hamlets, stations and landings, trails to the
pass shelters and landmarks, each a least-cost route smoothed into a curve
with its crossings recorded; every service road and track has a profile of
its own, and a sample by one takes its carriageway, its verge and its cut
and fill banks; a trail clears and wears its line; a river valley's walls
wander and, with a road's banks, are soil but where deep and steep. Built,
reviewed and the review's findings fixed the same day. **The world
moved** in step 6: everything pinned was blessed (below), `GENERATOR_VERSION`
once. Not committed, `just ci` not run.

### What was built: the network (step 5)

- **Which places are joined** (`hydrology/roads.rs`, in each mass's bake
  after its sites). Towns and the far end of Port Serein's road by a
  minimum spanning tree over the travel cost on the 512 m lattice the
  placement surveyed (each weighing its six nearest within 45 km), plus a
  loop wherever the detour through the tree costs over twice the direct
  link. Then each hamlet, station and landing, nearest first, by a track
  to wherever the network is cheapest to reach, usually a junction partway
  along a road; then each pass shelter and landmark by a trail the same
  way. Junctions are records of their own (`Network::junctions`,
  `RoadEnd::Junction`).
- **Port Serein's road** is kept whole as road 0 (its ends the city's exit
  and the road's far end, its traced points unchanged), so its ground and
  the city's fingerprint cannot move. A link to the city meets it anywhere
  along it: the backbone forks from it 330 m out of the exit.
- **Where a road runs.** A search on the 512 m lattice, then one on 64 m
  nodes in a corridor round that route (384, 256 and 192 m either side by
  class, widened 2.2 times only if nothing is found). The cost: length,
  gradient against the class's limit (8, 12, 22 %), climb, side slope,
  turning, a reversal's own price (300, 120 and 20 m by class, its share
  for any turn past a right angle), the river's meander belt, and a gentle
  value noise so lines wander on plains. The sea, lakes, standing water,
  other sites' pads and 100 m round Port Serein's extent are closed. A
  river is crossed only on an edge that crosses its channel once: a culvert
  for creeks under 2 m, a ford for tracks and trails at or under
  `FORD_DEPTH_M`, otherwise a bridge, dearer skewed, on a bend, at a mouth
  or between unequal banks. Travel on an existing road costs 0.12 of new
  ground, so later roads merge into earlier ones.
- **The line.** Resampled every 16 m and smoothed with its crossings, ends
  and approaches held, a point held where the search climbed near its
  limit, each kept within 1 m of its searched height; then a bend sharper
  than the class's radius tightened (each point moving at least half a
  pass's share, within 3 m of its height). A stretch that came to cross
  water, to cross a channel at a fall or under the search's least angle,
  or onto ground the search kept off (another site's pad, a lake's water,
  half the settlement's margin) is put back on the searched line, wider
  each round, then whole. A road ending on another is eased onto its line
  over the longest of 120, 60 and 30 m whose eased stretch stays clear the
  same way.
- **What is recorded** (`strata_worldgen::roads`). Points every 20 m on
  straights, closer in bends (at most 0.25 m off the curve), with the
  ground under each, and a point where each held stretch ends (a crossing's
  deck, an arm's level run off its junction). Every crossing along the
  final line with kind, position, bearing, span, deck, water surface, both
  bank heights and its abutments; banks are read on the road's own line
  and at a gorge's rim. A bucket index answers `Network::nearest_road` and
  `ProceduralWorld::nearest_road` (class, distance, bearing) without a
  sample. Roads and junctions are folded into the network's content hash.
- **The worldmap's `-roads` layer**: roads by class (red service, amber
  track, white dashed trail), junction dots, bridge bars, blue ford
  diamonds and grey culvert squares, the sites over them and a key;
  `--no-roads` leaves it out.

### What was built: the grade (step 6)

- **The profile** (`hydrology/grade.rs`, in the bake after the roads are
  recorded, `Road::profile`, a level and a rise per point, read between
  points as the cubic of both by `Road::level_at`). On a 4 m grid: the
  recorded ground averaged over 120 m (service) or 70 m (track); held to
  the pad's level at a site; at a junction level with the through road's
  surface out past its verge and the foot of its bank, measured square to
  it (`junction_hold`, at most 40 m along the arm), at its crown's fall
  there (roads are profiled after those they end on); to a bridge's deck
  from 6 m (4 m) behind each abutment, to a culvert's deck over its
  channel and 2 m either side, and to a ford's bed over its wetted width;
  never under the water of a lake whose bed the carve reads there, nor
  under a river's surface within 30 m of its banks (+0.5 m), across the
  road out to its banks' reach, but that floor never asks more than the
  grade of a held level (it rises from a pin at the class's grade, and
  after the vertical curves holds at the pin's level a curve's length
  out); graded to 8 % (12 %) by the mean of a forward-then-back and a
  back-then-forward pass, balanced six rounds against its target, let
  steeper only where that would cut over 18 m (12) or fill over 10 m (7);
  its rise smoothed over 48 m (28) within each free stretch, a held run
  lending its own rise to the window so a climb rounds into a deck. Trails
  and Port Serein's own road get none.
- **Crossings**: a bridge's deck stands its class's freeboard over the
  water (2.5, 2.0, 1.5 m) and half the banks' difference over the lower,
  3 m at most, so between unequal banks one approach comes up on a fill
  and the other down in a shallower cutting; each abutment where its bank
  first meets the deck, no further out than that bank's rim. A creek whose
  lower bank stands over 4 m above its water (a gorge) is bridged by a
  graded road rather than culverted or forded.
- **The ground** (`RoadGrid::shape`, in the sample after the carve and
  before a site's pad, from the cell the carve reads). A sampled 512 m cell
  gathers every segment whose ground can reach it into 32 m buckets; a
  point reads its bucket, and each road's segments within a blend of the
  nearest take a share, so the level and the distance pass round a bend's
  inside without a step. Roads are laid arms first, so the road an arm
  ends on has the last word on its own carriageway; past an arm's end on a
  junction its distance counts three times over, so its carriageway stops
  inside the through road's and does not round out onto the far side.
  Across: the profile less a crown (2.5 %, 3 % at the edge) carried over a
  2.5 m (1.5) verge; past the verge's edge, which wanders a metre either
  way, a cut bank of soil at 0.6 (0.65) steepening to the rock's 1.8 above
  3 m (2.5), or a fill at 0.5 (0.55) steepening to 1.0 after 4 m (3);
  shoulder and toe rounded; faded into the ground over the last 10 m of a
  40 m (30) reach. Every number carries its gradient. Under a bridge the
  ground stops at the abutments. A ford's road dips to the bed and the
  water stands over it; a culvert fills the creek. **A river's channel
  takes none of a road's ground but at the road's own crossings**
  (`grade::channel_share`): within the channel's half-width nothing,
  within its bank and 8 m more a fade, unless the point is along the road
  within a bank's reach of one of its crossings' held stretches (fading
  over 30 m more).
- **The record** (`Landform::cut_m`, the road's cut or fill, and
  `Landform::cleared`, its way; neither serialized): the carriageway is
  developed and laid in the city's setts (service), gravel (track) or worn
  soil (trail), its edge broken by a two-octave noise, and widened over
  its verge in the last 14 m of an arm into a rounded mouth;
  `ProceduralWorld::sample_without_roads` is every stage but the grade.
- **Placement** (`strata_flora::fit_at`): nothing on a carriageway,
  nothing but grass where `cleared` is over a half; a cliff kit's piece is
  refused on made ground steeper than 1.2.
- **The look** (`strata_terrain::mesh::made`, `made_rock`): a vertex whose
  ground was moved by over 5 cm to 0.6 m (the most of it and its eight
  neighbours) bares rock only where the cut is deep and steep (5 to 14 m,
  that band wandering 5 m either way with a 170 m noise; the slope 1.5 to
  2.2), only in the clusters that noise opens, and there in outcrops of a
  23 m value noise with a wide edge; a fill bares none.
- **Valley walls** (`carve.rs`): a wall's foot and top move in by up to 14
  m and its slope steepens by up to 35 % with a 90 m noise, held down by
  the rock's grain and gone at half of it or more (the folded belt).

### Where it left the design, and why

- **Rock is rare on made ground**, on the owner's wish that cuts read as
  hillsides: of the valley-wall ground within 25 km steep enough to bare
  rock by the terrain's rule (41.4 %), 1.2 % did as first built and 0.4 %
  does after the review's clusters and steeper band; of the road banks
  (58.1 % steep enough) 0.7 % and 0.2 %.
- **Water floors** were not in the brief: a road beside a lake cut its rim
  under the level and the lake test caught water standing in the cut.
- **The fog's** 128 m nodes read a road's cutting as a hollow and fill
  it, and a fill stands over the fog beside it; its test holds those
  points apart, to a bound of their own.

### The review's findings, and what became of them

- *Should: no gate held the 1.9 us bound where a graded road runs.*
  `tests/perf.rs` `a_road_adds_little_to_a_sample` now also samples 65 by
  65 grids along the graded road nearest the mountain strip (a track at
  (12 345, -17 778)) and holds the decision's 1.9 us there: 1.886 and
  1.887 us pinned to one core (1.751 and 1.755 without the grade). It
  passes by 13 ns. The road out of the city, over the order-2 river's
  valley, whose ground costs over the bound before any road, is held
  relatively (the grade 145 and 146 ns). The carve's own bound (450 ns)
  still fails, 478 ns measured under load; it is not loosened (below).
- *Should: an arm met its road with a lip.* An arm is held level with the
  through road's crowned surface out past its verge and the foot of its
  bank (`junction_hold`), with a point recorded where that run ends, and
  past its end its ground is squashed inside the through road's.
  `tests/grade.rs` `an_arm_leaves_its_road_at_that_roads_level` walks 43
  arms within 25 km every half metre for 40 m off the junction: the
  steepest 2 m is 0.127 (allowed the steeper class's grade and 0.06).
  Pinned at the centreline as before, the same test fails (0.174 on a
  service arm, allowed 0.14). The junction check of the profiles now also
  covers arms onto Port Serein's own road, against its recorded ground
  (the review's nit).
- *Should: a ford's approach jumped to the water's floor.* The floor is
  capped at each pin's level plus the grade's climb from it (and, after
  the curves, at the pin's level a curve's length out). `tests/grade.rs`
  `a_fords_approach_holds_its_grade`: at each of 184 ford edges within 60
  km the profile within 12 m climbs no steeper than the grade, or than it
  climbs further out, by more than a fifth (the cubic's overshoot between
  points). Without the cap the test fails, 0.771 at a track's ford.
- *Should: the carve's guards skipped any ground a road moved.* They skip
  only within half a crossing's span and 50 m of where it meets the
  channel (`by_a_crossing`), and the natural-ground check compares the
  ground without the roads, to the bit, everywhere. Narrowed, the guard
  caught a track's fill 2.3 m over a meander's water at (8 966, -7 191),
  50 m from its ford: the fix is the channel closure above. All 17 carve
  tests pass.
- *Should: the fog's gate was loosened to 130 m of any road.* It now
  skips only a point on a road's fill or with a road's cut under one of
  the four 128 m nodes round it: 19 points of 3 600, at most 5.66 m, held
  under 6 m as a known gap like the valley's; the rest of the plain at
  most -0.02 m against its 5 m.
- *Should: the line was not checked after smoothing and easing.* Above
  (the line): pads, lakes, half the settlement's margin, channels at a
  fall or too skew, and the ease's three lengths. `tests/roads.rs`
  `no_road_on_another_sites_pad` checks every point within 25 km.
- *Should: valley walls read as ruled quarry faces.* The made rock's depth
  band wanders, only clusters of a 170 m noise bare any, the slope band is
  1.5 to 2.2 (a road's bank at its rock's slope only where steepest, so an
  outcrop is a face and not a disc of rings), and an outcrop's edge fades
  over a wider band. See the captures.
- *Should: mountain tracks were sawtooth scribbles.* A reversal costs the
  search 120 m on a track (300 service, 20 trail), so a climb winds in
  fewer, longer legs whose tips turn over two or three moves, and the
  tightening moves a sharp bend's points at least half a pass and within
  3 m of their height. The track to Vokun Monitor now climbs in a handful
  of switchbacks with legs of a few hundred metres, most tips rounded
  (`review-hairpins-roads.png`, `review-mountain-roads.png`,
  `review-switchbacks.png`, against `dbg-mountain-roads.png`). Some tips on
  slopes steeper than about 0.3 are still a V tighter than 25 m: the
  height tolerance holds them.
- *Should: the bake and the handshake grew past their targets, ungated.*
  `tests/perf.rs` `the_bake_and_the_version_stay_within_what_the_roads_cost`
  holds Port Serein's mass's bake under 4 s and `world_version` under 600
  ms, where the review left them (3.51 to 3.55 s and 480 ms), so they grow
  no further unseen. Meeting the targets is the owner's (below).
- *Nit: a bridge's deck at the lower rim.* Half the banks' difference, 3 m
  at most, over it; the deepest service cut by a bridge is still 28 m at
  (-23 727, 26 018), a gorge whose rims differ by far more.
- *Nit: the junction check skipped Port Serein's road.* Fixed (above).
- *Nit: `world_version` did not cover the grade's ground.* Ten samples
  across the canary window's first graded road, their height, cut, cleared
  and developed shares and material, join the hash.
- *Nit: crossing kinds the search did not pay for.* Not changed: a gorge's
  culvert becomes a bridge at record time, and a trail's "bridge" is
  walked through as ground until step 7's footbridge. Step 7 should know.
- *Nit: ribbons, square junctions, grey setts.* An arm's carriageway
  widens into a rounded mouth; the setts' colour and the gravel-to-setts
  change at the fork are not changed.
- Found on the way: a bridge's banks were read on a straight line off the
  crossing, and where the road came along a terrace below a gorge's wall
  the deck stood 30 m over it (a track's 30 m fill at (3 407, 50 913)).
  They are read on the road's own line now.

### The measures (seed 42, release, 16 threads unless said)

- **The network** (Port Serein's mass, 92 074 km2): 5 258 roads, 23 927
  km, 5 277 junctions; 14 of 5 232 sites unreached, none within 60 km.
  Kilometres (service / track / trail) and crossings (bridges / fords /
  culverts): within 12 km 13.5 / 46.2 / 2.3 and 1 / 8 / 5; within 60 km
  541.8 / 913.6 / 209.6 and 77 / 105 / 107; within 200 km 4 461 / 8 975 /
  2 205 and 577 / 658 / 860. Within 60 km 8.6 % of road runs within 150 m
  of a river. The ground under the routes climbs over the class's grade,
  at a profile's scale (100 m, ground averaged over 40 m), on 6.8 / 13.1 /
  17.1 % of their length (step 5 as first built: 6.7 / 13.5 / 16.5).
- **Routes from the city**: to Cecautint (14.2 km away) 15.2 km over 10
  roads, climbing 248 m, crossing a culvert and two bridges; to Wack's
  Hold (3.6 km) 4.1 km, no crossing; to Cebur Rim (9.7 km) 10.7 km. The
  steepest ground under a route, 53 % over 100 m, is the backbone's climb
  out of its first river's valley at (7 773, -5 149), which the profile
  cuts at 8 % (below).
- **Profiles within 60 km** (every 5 m, bridges' gaps out): service 539
  km, 76 % of it in cut (78 % as first built), cut median 0.53 m (0.69),
  90th percentile 2.29 (3.0), deepest 28.0 at (-23 727, 26 018), a gorge
  bridge's approach; fill median 0.43, p90 2.34 (3.5), highest 14.9;
  123 cuttings over 5 m (21.1 km; 142 and 25.9), 13.1 km over 10 m; 1.6 %
  of its length over its grade (2.0). Tracks 914 km, 66 % in cut, cut
  median 0.40, p90 6.33, deepest 23.3; fill median 0.55, p90 6.81, highest
  15.9 (30.0 before the banks were read on the road's line); 381 cuttings
  over 5 m (68.0 km), 45.2 km over 10 m; 6.9 % over (7.8). The sampled
  centreline stands on its profile (median 0, p99 6 and 12 mm); at four
  points it is 1.7 to 6.6 m off, where another road's bank takes it.
- **The bake**, Port Serein's mass on 16 threads: 3.51 to 3.55 s, roads
  1.91 to 1.94 s of which the profiles 0.14 s (3.46 to 3.52 and 1.86 to
  1.90 as first built; 1.49 s with no roads). `world_version` 480 ms (450
  as first built, 223 before the roads). The game's startup bake within
  128 km was 6.58 s at step 5 and was not measured again.
- **The sample**, pinned to one core: by the road out of the city 2.18 us
  against 2.04 without the grade (the grade 145 ns); by the graded track
  nearest the mountain strip 1.886 us (1.75 without), the bound 1.9;
  mountains 1.749 us, lowland 1.125; the carve on the river strip 478 ns
  against its 450 bound (load 9 to 11 by another session's work; 466 to
  476 as first built).
- **Made ground**: of the valley-wall ground within 25 km steep enough to
  bare rock 0.4 % does; of the road banks 0.2 %.

### Tests

`tests/roads.rs` (8, release, 7 s): the canary island's roads identical
on 1 and 4 threads; every site within 60 km reached; within 25 km no road
point in a lake, over the sea, inside Port Serein's extent (but its own
road) or on another site's pad, and none in river water away from a
crossing; every meeting of a road and a river channel (walked every 2 m)
a recorded crossing; the over-grade share at a profile's scale under 8 /
16 / 19 % (service / track / trail); no two roads side by side for over
200 m within 100 m; both queries agreeing with a brute-force search.
`tests/grade.rs` (6, release, 8 s): every profile within 25 km meets its
pads, junctions and decks and stays within its grade (under 4 % of
service, 10 % of tracks over); an arm leaves its road at that road's
level; a ford's approach holds its grade; the ground walked every 5 cm
across roads, along them, through their junctions, onto their pads,
through every bridge's abutments and every ford is continuous with its
carried gradient; under a bridge the carve's ground to the bit; far from
roads and in Port Serein the ground to the bit. `hydrology::grade` (4 and
a timing). `tests/perf.rs`: `a_road_adds_little_to_a_sample` (with the
mountain road's absolute bound) and
`the_bake_and_the_version_stay_within_what_the_roads_cost`, both by hand.
`strata-flora` `nothing_grows_on_a_carriageway`. The carve's three water
guards hold road ground but by a crossing; the sites' pad walk skips road
ground and its no-sites comparison uses `sample_without_roads`; the fog's
test holds a road's points to their own bound. Probes:
`tests/roads_measure.rs`, `tests/grade_measure.rs` (with `probe_crease`,
`probe_profile`, `probe_site_named` and `probe_showcase`), `strata-terrain`
`tests/made_ground.rs`.

Run, one binary at a time, four test threads, release unless said: the
worldgen library (96, and in debug), `roads`, `grade`, `carve`, `sites`,
`version`, `snapshots`, `drama`, `geology`, `continuity`, `port_serein`,
`determinism`, `coverage`, `rainshadow`, `perf` pinned by hand;
`strata-weather` (and its pins in debug); `strata-flora`; the
`strata-terrain` library; `strata-physics` `headless`; `worldmap`.
`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
and `just doc` clean. Not run: `just ci`, `just test`, `just server-check`,
the benches.

### What was blessed

- `GENERATOR_VERSION` 14 to 15 (step 6, once).
- `world_version` `0x7a48_c3e0_6fb9_5fb9` (before the roads) to
  `0x3235_12a7_7f98_94aa` (step 5) to `0xb032_9121_da5e_9251`
  (`0xbdcc_83e3_c638_eb0b` as step 6 was first built).
- The canaries to `[0xdeb6_e6eb_f257_c640, 0xda36_6cc7_bada_9a34,
  0x2354_d442_ee40_0b10, 0x2b41_28ab_628d_b2ed]` (step 5's
  `[0xd399_40c2_2c0b_fbc3, 0xf2c8_3696_2399_2d77, 0x4982_c8cd_90af_fa05,
  0xc44e_76e5_b72a_391f]`, step 6's first `[0x28de_8e04_d0e1_8542,
  0x64f5_9fc4_ea6f_4fb8, 0x5eec_203c_3db2_a67a, 0xce98_0abd_c4d7_4206]`).
- `seed_42_samples`: the version line only.
- `drama::seed_42_measures`: the folded belt 2.01 (gate 2.0), basalt 1.81,
  sandstone 1.28; slope classes and the profiles' heights within 0.2 %.
- The world without geology `0x5420_0e26_53c3_487c` (step 6; the review
  did not move it).
- The fog: the bay tile `0x46ee_4e77_acb9_c3ed`, its nodes
  `0x1914_ca40_e209_dbc9`, between them `0x4075_c179_65a6_cd48`, the same
  in a debug build.
- Port Serein's fingerprint 6857689028393273736 holds (roads, sites and
  carve tests), and its extent's ground matches the ungraded sample to the
  bit.

### Captures

`scratch/review/roads/review-*.png` (flycam, `--fog 0 --time 0.45`, 1 500
frames, the poses of the session's `grade_shots.json`): `exit-eye` and
`exit-80` (the city's road and the fork), `crossing` and `crossing-side`
(the backbone's first bridge, 9.3 km out at (7 579, -5 029): two level
shelves, the valley soil under them), `cutting-eye`, `cutting-above` and
`deep-cutting` (its descent into that valley), `junction` (a track leaving
it at (2 180, -2 862)), `hamlet` (Wack's Hold), `trail` (to Cebur Rim),
`ford` (a track's ford at (-1 431, -4 503)), `valley`, `town` and
`switchbacks` (the track to Vokun Monitor from 260 m). The worldmap:
`review-ps12-*`, `review-ps60-*`, `review-ps200-*` (the city's square at
12, 60 and 200 km), `review-mountain-*` (8 km round the mountain track) and
`review-hairpins-*` (2.5 km). The first build's are `grade-*`, `ps*-roads`
and `dbg-*` beside them.

### What is left

- **For the owner.** The bake of Port Serein's mass is 3.5 s against about
  3 s; the game's startup bake within 128 km was 6.6 s at step 5 (2.8 s
  before the roads) and was not measured again; `world_version` takes
  about 480 ms against 223 ms before the roads, because its canaries lay
  their roads on one thread. A smaller startup radius, a cheaper search
  sample or canaries without roads (and a road canary of its own) are the
  ways; the gates hold where it is now. The carve's 450 ns bound still
  fails (466 to 478 ns, 454 to 469 without the walls' wander): move the
  bound or claw it back. The mountain bound by a road holds by 13 ns.
- **Cut against fill** on service roads is still mostly cut (76 % of the
  length, 4 to 1 by area), mostly under a metre. Deep cuttings remain
  where a route goes straight down a valley's wall: the backbone's descent
  to its first bridge is a cutting 600 m long and 20 m deep at its
  deepest, and its climb out on the far side 22 m. The grade holds 8 %
  there because the cut stays under 18 m most of the way; a lower
  `cut_most_m` would steepen it instead, and winding the route down the
  wall is the network's. The owner may prefer either.
- **Over grade**: 1.6 % of service and 6.9 % of tracks, at summit stations
  and gorges (a track goes straight down a 128 % gorge wall to a ford at
  (-25 951, 27 231), the search finding nothing cheaper in its corridor).
- **Switchback tips** on slopes over about 0.3 turn tighter than a
  track's 25 m.
- **Roads that cross without a junction**: at four of some 30 000 points
  within 60 km the sampled centreline stands 1.7 to 6.6 m off its profile,
  where another road's bank takes it.
- **The look**: setts read grey from altitude; the city's gravel road
  meets the setts at the fork; carriageway edges are straight from 50 m.
  Cliff kits stand on natural steep ground beside roads.
- **For step 7 (the crossings).** `Network::roads()` gives each road's
  `crossings` in order along it: `Crossing { kind, at, along_m, bearing,
  span_m, deck_m, surface_m, banks_m, width_m, depth_m, river,
  abutments_m }`. `bearing` is the road's unit direction from its first
  end; `abutments_m` the distances along the road where each bank meets
  the deck (a bridge's, a culvert's) or the wetted edges (a ford's).
  The ground: a bridge's profile is held level at `deck_m` from
  `abutments_m[0] - shelf` to `abutments_m[1] + shelf` (shelf 6 m service,
  4 m track, `grade::way(class).shelf_m`), so each abutment stands on a
  level shelf of the carriageway's width; between the abutments the ground
  is the carve's, falling from each shelf's edge at six times a fill's
  slope (`ABUTMENT_FACE`), so the deck spans `abutments_m[1] -
  abutments_m[0]` over open ground with the river under it; the deck is
  the class's freeboard over `surface_m` (2.5, 2.0, 1.5 m) and between
  unequal banks up to 3 m over the lower. A culvert's road is filled at
  `deck_m` over the channel's width and 2 m either side; its pipe goes at
  `surface_m`. A ford's road dips to `deck_m` (the bed) over its wetted
  width, the water over it. A trail gets no profile: a trail's `Bridge`
  over a river deeper than `FORD_DEPTH_M` is ground the footbridge must
  span, its `deck_m` its class's freeboard over the water. A creek in a
  gorge on a graded road is a `Bridge` whose banks (and so `deck_m`) are
  the rim's, and its span long. The server and the client should place
  the kit from these records as they place the site groups (from the
  network each bakes, `strata_sim::sites`), colliders on the server.
- **The showcase stretch.** Port Serein's road (road 0) runs from the
  exit (-646, -922) to its head (-1 214, -1 597); the backbone (road 54, a
  service road of 14.8 km) forks from it 330 m out at (-572, -1 218), runs
  east past the junction at (2 180, -2 862), and 8.7 km out enters its
  first cutting at about (7 180, -4 900), 20 m deep at its deepest at
  (7 405, -5 043), to its first crossing 9.3 km out: a bridge at (7 579,
  -5 029), span 23.6 m, deck 68.5 m over water at 66.0. A culvert follows
  at (7 702, -5 078) and a 22 m cutting out of the valley at (7 759,
  -5 135); the next bridge is at (11 934, -7 390), span 48 m.

## Step 7, as built (2026-10-05)

The crossings: a bridge kit in `veyra-buildings`, baked in span classes by
the content step, stood at every bridge, culvert and ford by a pure function
of the network's crossing record shared by the server and the client, with
colliders on both. Built, reviewed and the review's findings fixed the same
day. **The ground did not move**: no change to the sample, no
`GENERATOR_VERSION` bump, nothing pinned blessed. Not committed, `just ci`
not run.

### What was built

- **The kit** (`veyra_buildings::bridge`, Bevy-free). A road bridge of one
  class is one file: a precast deck in slices (each its own convex proxy, so
  the space under it stays open), cast kerbs, painted steel rails on both
  sides (one proxy a side from inside the deck to 1.15 m), and a stone
  abutment at each end with a cast seat, a pilaster outside each rail and a
  wing wall flaring back into the road's bank. Up to 24 m clear the deck's
  soffit is a low arch (rise 5.5 % of the span, at most 1.1 m on a service
  road, 0.7 m on a track) springing from the seats; past it the deck rides
  on steel plate girders over piers, a pier a piece of its own (cast cap,
  masonry shaft with cutwaters up- and downstream, battered out across the
  road, thicker along it as it is taller: 1.6 m at 6 m, 3.0 m at 48 m on a
  service road) baked 6, 12, 24 and 48 m tall. A footbridge (trails) is
  boards between two Warren pony trusses that are its rails, on small stone
  sills. A culvert is a stone headwall with a 2 m wide box's mouth, a cast
  frame and apron, dark inside, and wing walls whose tops fall with the
  fill's toe, baked for boxes 0.45, 0.65 and 0.85 m tall; a ford gets
  banded steel depth posts on feet set flush in the bank; a trail's ford
  stepping stones (five shapes, a low dome 12 cm over the water widest just
  under it and narrowing to a foot 1.1 m down). The deck's top is level 2 cm
  over the road's level and falls 12 cm over its last 1.2 m into the shelf,
  so no end has a lip. Slots bind `bridge:masonry` (the library's ashlar
  blocks, darker and weathered, at three quarters of their repeat, with the
  slow world-space variation) and `bridge:ford-stone` (the rock kits'
  granite darkened), both in `veyra_materials::crossings`, and the city's
  pad concrete, grid concrete, painted and dark steel, hazard paint, recess
  and boards.
- **The classes.** Clear spans every 4 m from 4 to 64 m for service roads
  and tracks, 4 to 32 m for trails: 40 bridge files, 8 piers, 3 headwalls,
  a post and 5 stones, 57 files and 9.3 MB under `generated/crossings/veyra/`,
  and the layout `crossings/veyra/kit.ron`
  (`strata_worldgen::crossings::CrossingKit`, with the deck's end
  (`DeckEnd`), each pier's half thickness and each headwall's box and
  coping). A class serves gaps (between the record's abutments) that put
  each face 0.25 to 2.25 m out into the gap; the deck runs on 2.5 to 4.5 m
  past each gap edge (`PAST_EDGE_M`), bedded in the shelf (6 m service, 4 m
  track). The classes tile every gap from 3 to 68.5 m (36.5 m for trails).
- **Placement** (`strata_worldgen::crossings::place_crossing`, a pure
  function of the record, the layout and the world's ground). A bridge takes
  the shortest class whose gaps hold its gap, centred on the chord between
  the road's centreline at its two abutments, turned along it, the deck's
  top at `deck_m`. On a graded road the ground is read under both ends of
  the deck, at five points across the carriageway, at the end, at the
  ramp's top and 1.5 m further in where that is still short of the gap: it
  must stand within 0.25 m under the deck's top and 0.4 m over it. If it
  does not, the next class up (whose ends reach 2 m further) is tried, and
  if that misses too the crossing is `Unplaced::Unmet` and stands nothing.
  Its piers are spaced evenly, then shifted together by the least (in
  0.25 m steps, up to 0.45 of a spacing) that keeps each clear of the
  channel's water (its width along the road, as skew as the creek's flow
  crosses it, and the chosen pier's own half thickness) with no girder over
  27.5 m (`GIRDER_MOST_M`); each as tall as the girders stand over the bed
  plus a metre, from the four heights. A culvert's headwall at each side is
  looked for walking out along the creek's flow from past the road's
  carriageway, its verge and the headwall's own depth, where the ground
  first stands under the creek's own water (water standing higher than the
  crossing's by more than 0.5 m and 0.15 m a metre out is another
  channel's), within 30 m or not at all; its box is the tallest whose
  coping stays under the road's level, and a side where even the lowest
  would stand over the road gets none. A ford's posts stand on the ground
  0.7 m past the way's edge at both wetted edges; a trail's ford and a
  trail's creek (where the trail's ground is the creek's) get a row of
  stones 1 m beside the trail, about 0.95 m apart, seeded by the crossing's
  id. `ProceduralWorld::crossings_near` lists crossings with an id (mass,
  road, index).
- **At run time.** `strata_sim::crossings` reads the kit (`Crossings`,
  published with the world) and keeps `NearCrossings` within 5 km of the
  observers on the task pool, as the sites' list is kept, and warns once
  per crossing that stands nothing where something was meant to (all but a
  trail's creek in a gorge); the physics builds a static body per piece
  within 600 m (released past 800), the site colliders' streaming now a
  shared `Streamed` helper that borrows each piece and makes its marker
  only for a body it spawns; the client draws within 4 km (dropped past
  4.4), copying nothing for a piece already drawn. Nothing was added to the
  replication protocol.
- **The handshake.** The site groups' version now also hashes the kit's
  layout and every file it names, last, so a client with other bridges is
  refused as one with other site groups is; the field and the function keep
  their names (the handshake is append-only) and their docs say so.

### The review's findings, and what became of them

- *Must: a deck's end had a lip, or its corners floated, at about one
  bridge in ten.* The grade's shelf is level across the carriageway only
  from about 2.5 m back from the gap's edge (the abutment's face, rounded in
  plan, falls away at the kerbs nearer it), and the deck ran 0.75 to 2.75 m
  past the edge. It now runs 2.5 to 4.5 m (`PAST_EDGE_M`), and placement
  reads the ground under both ends (above). Measured with the review's
  probe against the real ground (deck top at the end minus the ground, at
  0 and 3.5 m either side): within 60 km no step over 5 cm (8 of 77 before,
  4 over 0.25 m), the worst -0.02 m (the deck bedded); within 200 km 5 over
  5 cm of 530 placed, the worst 0.14 m (57 of 546, p99 1.0 m before). The
  ground over the kerbs' outer corners: p99 0.09 m, at most 1.36 m buried
  on a curve. The review's cases: (50 982, -2 380) and (52 128, 17 312) are
  bedded; (-42 005, 2 155) takes the next class and is bedded; 189/0 and
  189/1, 36 m apart on one road with decks 20 m apart, and (16 376, 88 960)
  on a curve, are `Unmet`. `crossings::tests::a_deck_whose_ends_miss_the_ground_takes_the_next_class_or_none`.
- *Must: a culvert's headwall stood in the carriageway.* The search starts
  past the carriageway, the verge and the headwall's depth, and only the
  crossing's own water counts. Within 200 km no headwall's back is in the
  carriageway or its verge (71 and 91 before); a flooded road's edge, whose
  water record stands at the road's level, no longer reads as the creek.
- *Must: the red patch.* Traced and fixed; it was not the kit. Bevy 0.19.1
  keeps a light's texture in its clustered decals' table, and the cloud
  shadow (`strata_atmosphere::clouds`, the sun's `DirectionalLightTexture`
  through `shadow_projection`'s map) was being blended as a decal over
  standard materials wherever a fragment's light-space point fell inside
  the decal's unit box: with that map's third row along the sun, a metre
  thick slab square to the sun through the render origin, inside a quarter
  of the cloud tile, sliding with the clouds. There the mask, a
  one-channel image read as `(r, 0, 0, 1)`, replaced the base colour with
  red: on the rails, posts and stones (plain standard materials) where the
  slab crossed them; it missed the deck and the masonry in the frames
  seen. The slab's slide is why the patch came and went
  with the frame and the view, and why `--no-foliage`, which changed the
  frame rate, seemed to cure it. Bisected by holding the frame and taking
  things away (shadows, SSAO, probes, local fog, light rays, the
  environment map's hold, wind, rivers, trees, grass: all still red; clouds
  off: none). The fix moves the map's `local.z` 100 km out
  (`OUT_OF_DECAL_BOX`), which the light's own lookup does not read; the
  shadow test checks no point is in the box, and the cloud shadow's uv is
  unchanged within its tolerance. At the review's pose, frames 1 300 to
  1 600, 0 red pixels against up to 1 500 before
  (`bridge-red-before.png`, `bridge-red-after.png`).
- *Should: an unplaced crossing was silent.* Warned once per crossing id
  (above); `Unmet` is reported too.
- *Should: headwalls over the road, boxes that could not pass under it.*
  Three box heights, the tallest whose coping is under the road. Within
  200 km no coping is over the road (939 of 1 417 before, p50 -0.26 m
  now) and no box lacks 0.2 m of slab under it (759 before). The cost:
  where a creek stands within a metre of the road, mostly upstream, there
  is no headwall. Within 60 km 108 of 184 sides have one (10 culverts none,
  57 one, 25 two; 20 sides dry within 30 m, 51 with the road under the
  lowest coping, 5 with only the road's own water); within 200 km 873
  headwalls at 787 culverts (92, 517, 178).
- *Should: the physics tests ran a minute each in `just test`.* The three
  on the real world are ignored with the release command in the module
  doc (18 s together in release); the missing-file one stays. The sim's
  server-and-client test is ignored the same way (20 s in release).
- *Should: stepping stones read as teeth.* Five shapes, a stride apart, low
  domes narrowing below the water, darker stone. See `bridge-trail-stones`.
- *Should: the stone read as toy bricks; the 44 m class's pier a pencil.*
  `bridge:masonry` (above) and piers thicker as they are taller. Better,
  not done: see what is left.
- *Nit: a headwall in another channel.* Only water within 0.5 m (and 0.15 m
  a metre out) of the crossing's own counts.
- *Nit: string clones per piece per frame.* Gone, in the physics and the
  client.
- *Nit: the trails' gaps.* 36.5 m, as the kit serves them (above).
- *Nit: the version's name; the layout mapping duplicated.* Docs widened;
  `GIRDER_MOST_M` is the kit's one number; the physics test's mapping stays
  (a physics test cannot depend on the content step) and says so.
- *Nit: two captures did not show their subject.* Retaken.

### The measures (seed 42, release)

- **Gaps between abutments within 200 km.** Service 454 bridges: least 3.7
  m, p10 12.4, median 15.4, p90 45.0, p99 110.8, longest 147.0; by 4 m: 241
  at 12 to 16 m, 76 at 16 to 20, 36 at 20 to 24, 18 at 24 to 28, then
  thinning out. Tracks 122: median 20.9, p90 69.0, longest 221.8, most of
  them gorges (deck over the water: median 20.9 m). Trails 1 (27.5 m).
  Within 60 km: service 70 (median 15.4, longest 57.0), tracks 7.
- **Placed.** Within 200 km 530 of 576 graded bridges and the trail's one:
  service 428 of 454 (14 gaps over 68.5 m, 2 gorges deeper than the 48 m
  pier, 10 `Unmet`), tracks 102 of 122 (14 over 68.5 m, 6 `Unmet`). Within
  60 km 75 of 77 (189/0 and 189/1 `Unmet`, 56 km out). The road stands
  over 1 m off the deck's axis over the deck at 11 placed bridges (6.3 m
  at most, a 63 m track gorge on a curve).
- **Culverts and fords.** Above for the headwalls. Trails' creeks: 62
  placed (a few with every stone on dry ground, so none), 11 in a gorge
  (`NoWater`). All 509 tracks' and 149 trails' fords
  marked.
- **Round Port Serein.** Within 5 km of the city 3 crossings (a track's
  culvert, two track fords); within 5 km of the showcase bridge 7 (two
  service bridges, a service culvert with no headwall, a track ford, a
  trail's creek and two trail fords). Placing all 2 095 crossings within
  200 km on 8 threads with the probe's ground reads: under 20 s with the
  bake; the placement alone was 0.2 to 0.4 s on one thread as first built
  and reads at most 60 more samples a bridge now.
- **Cost.** A 20 m service bridge (the showcase's): 1 836 / 648 / 608
  triangles by level, 0.2 MB; the 44 m class 2 684 / 624 / 488 and each
  pier 64; a headwall 136 / 136 / 12 to 44; a post 108 / 84 / 56; a stone
  88 to 96. Entities and convex parts as first measured (a bridge about 30
  near, 30 parts in one compound body; the 43 pieces round the showcase
  about 180 entities near, at most 94 parts).

### Tests

`veyra-buildings`: `bridge::tests` (every piece builds with its own name
and only the kit's bindings; the classes tile the gaps; the soffit clears
the water by the freeboard less 0.8 m, 0.4 m on a footbridge) and
`tests/bridge.rs` with the kernel's derived proxies (the deck level end to
end with no step and its ramps, the rails one proxy a side the deck's whole
length from inside the deck to 1.15 m, the deck bearing on both seats and
the abutments reaching down, the girders on the seats at the pier's cap,
every pier its height, the small pieces colliding, each headwall's coping
where the layout says). `strata-worldgen` `crossings::tests` (5: the class
chosen covers the gap and is centred and turned; a deck whose ends miss the
ground takes the next class or none; a pier keeps off the channel and
reaches below the bed, a gorge too deep gets nothing; headwalls past the
verge where the fill meets the creek, facing out, the tallest box under the
road and none under a low one; posts, and stones a stride apart the same
every time). `veyra-materials` (the crossings' definitions validate).
`content` `tests/crossings.rs` (every binding in the shipped library; the
layout names every file, tiles the gaps, and has headwalls in more than one
height). `strata-sim` `a_missing_kit_stands_nothing`, the version test
covering the kit and its files, and, ignored and run in release,
`a_server_and_a_client_stand_the_same_crossings`. `strata-physics`
`tests/crossings.rs`: a missing file collides nothing; ignored and run in
release, a character walks and then runs across the showcase bridge, is
stopped by either rail, and wades the track's ford at (-1 431, -4 503).
The game's `crossings::tests`. `strata-atmosphere` `clouds::` (13, the
light texture never inside the decal box). Probes:
`strata-worldgen` `tests/crossings_measure.rs`; `content`
`rebake_crossings` (ignored) writes the crossings' files alone; the review's
probe and its culvert diagnosis are in the session's scratchpad.

Run, one binary at a time, four test threads: those above.
`cargo fmt` on the changed packages, clippy with warnings fatal on
`veyra-buildings`, `veyra-materials`, `strata-worldgen`, `strata-sim`,
`strata-physics`, `strata-atmosphere`, `strata-net-replicon`, `content`,
`game` and `server`, rustdoc on the changed crates; `cargo tree -p server`
finds no authoring crate and no render crate. `just content` once. Not run:
`just ci`, `just test`, `just server-check`, the benches, an end-to-end
server and two clients.

### Captures

`scratch/review/roads/` (flycam, `--fog 0 --time 0.45`, 1 500 frames):

- `bridge-showcase-bank` (from the bank at eye height), `bridge-showcase-road`
  (from the road across the deck), `bridge-showcase-under` (at the water
  under the arch), `bridge-showcase-80` (80 m up beside it), `bridge-next`
  (the 44 m class and its 24 m pier over the gorge at (11 934, -7 390)),
  `bridge-culvert` (the track's culvert near Wack's Hold, its one headwall),
  `bridge-ford-track` (the track's ford there), `bridge-trail-stones` (a
  trail's stones at (9 024, -7 175)), `bridge-red-before` and
  `bridge-red-after` (the red patch's frame at the under-arch pose, before
  and after the cloud shadow's fix).
- **The walk**, `showcase-01` to `showcase-12`: looking back at Port Serein
  along its gravel road from 8 m up; at the fork; the backbone through
  wooded ground; at the junction (2 180, -2 862); entering the first
  cutting; in it at its deepest; the bridge coming into view (from the
  cutting's rim: from the road a placed rock stands between it and the
  bridge); on the deck looking down the river; the culvert at (7 702,
  -5 078); the cutting out of the valley; the whole crossing from 120 m;
  arriving at Wack's Hold along its track.

Looked at as a sequence, it delivers the shape of "a road that winds out
of the city, follows a valley, crosses a river on a small bridge, cuts
through a hillside, and arrives at a place", and the bridge itself, close
to, reads as a country bridge: an arched deck on stone abutments with
pilasters and wings, rails, bedded in the road. What lets it down most is
the road round the crossing, not the crossing: from 120 m the backbone
reaches the river along the valley side, turns a right angle down to the
bridge and another right angle off it, so the crossing reads as a dog-leg
the search made square to the water; the cuttings either side are raw
uniform walls of soil; and the ground is one brown. The culvert shows
nothing at all (its creek is buried, below), the city's road is gravel to
the fork and setts after, and the views from the road toward the bridge
are blocked by a placed rock pillar beside it. The walk does not follow a
river up a valley for long: the backbone meets its first river 9 km out,
over plateau and woodland before that.

### What is left

- **For the lead.** The crossings that stand nothing (16 `Unmet`, 28 gaps
  over 68.5 m, 2 gorges over the 48 m pier within 200 km; none within 55
  km) are the grade's and the network's to meet: a profile that cannot
  hold two decks 36 m apart and 20 m different, curves across a gorge, and
  gaps a span class would need trusses or more piers for. The road at a
  crossing turns square to the water at both ends (above); a skew bridge,
  or a search that pays less for skew, is the network's. Whether a
  culvert with a mouth on one side only, or none, is acceptable, or the
  grade should let a culvert's creek through its banks (the showcase's
  culvert has none for that reason) and hold the road a metre over it.
  Cliff kits beside roads block the view of a bridge.
- **The look**, still. The masonry is better (darker, smaller blocks,
  varied) but still clean, regular blocks; the arch's soffit draws olive in
  the ground's bounce light; the deck is one pale concrete; nothing ties a
  bridge's stone to its province's rock; the girder class still reads as a
  steel viaduct.
- **Footbridges' ends** are not graded and not read: the trail's ground
  meets them as it was. **Overlapping crossings** (a trail's ford 8 m from
  a track's at (9 024, -7 175)) each stand their own posts.
- **For step 8** (rivers and roads painted at distance, falls, roadside
  markers and lamps, wet banks and reeds). The crossings draw to 4 km and
  their far level keeps the deck, girders, piers and abutments; a road
  painted at distance meets the deck at the same `deck_m`, and the paint
  should stop at the deck's ends (`DeckEnd`, the class's `length_m`, the
  chord the deck is turned along) rather than under it. Markers and lamps
  belong at a bridge's ends past the pilasters, and the ford posts are the
  first roadside markers already (a ford's wetted edges, `abutments_m`).
  Reeds and wet banks must keep off a ford's way and a culvert's apron, and
  out from under a deck's soffit (the gap between the abutments). The
  `Streamed` helper in the physics and `pieces_within` are what a
  roadside prop would reuse. A standard material in a metre-thick slab
  round the render origin would have been painted red by the cloud shadow
  before the fix: anything new drawn with a plain `StandardMaterial` is
  safe now, but a material reading clustered decals should be looked at
  under the clouds at frames 1 300 to 1 600 of the showcase's under-arch
  pose.

## Alignment pass, as built (2026-10-05)

An alignment pass on the road network and nothing else: the showcase walk
out of Port Serein (seed 42) showed the backbone (road 54) reaching its
first river along the valley side, turning a right angle down to the
bridge at (7 579, -5 029) and another off it, and getting down the wall in
a 600 m cutting 20 m deep, going straight down at the class's 8 % limit;
service roads were 76 % in cut. The goal was a road that swings onto a
bridge, and runs along a wall rather than down it. **Partly met** (the
network is better, the showcase's own crossing is not). The world moved:
`GENERATOR_VERSION` is 16 (the step 5/6 value was 15), pinned values
blessed below. Built, reviewed, the review's findings fixed, the same day.
Not committed; `just ci`, `just test` and `just bench` not run (the
machine's CPU faults under load: targeted binaries only, two test
threads, release).

### What was built

All in `hydrology/roads.rs` unless said.

- **Turns near a river cost more** (`Builder::search`). A node within
  150 m of any river too wide for a culvert carries `NEAR_RIVER`; a turn
  there costs 400 m a right angle on a service road and 200 m on a track
  (20 and 10 elsewhere). A turn spread over several nodes is cheaper, so
  the smoothing can round it into a curve. Hairpins on graded classes cost
  4x (1 200 m, 480 m): without that the earthwork term produced sawtooth
  zigzags of 60 m legs.
- **Bridges may stand skew** up to 45 degrees (`BRIDGE_LEAST_SQUARE` 0.7;
  culverts and fords keep 0.42), and skew lengthens only the span part of
  a bridge's price, not its fixed cost.
- **Earthwork is priced** on service roads and tracks: the rise or fall
  beyond what the grade allows, less 2 m, as a wedge (depth squared over
  twice the grade; weight 1.0 service, 0.5 track), discounted like travel
  on an existing road. A crossing edge is priced as a descent to the deck
  and a climb out, so coming straight down a wall onto a bridge is dear.
  The side-slope cost of graded classes fell to 0.4 of what it was so a
  line can run along a wall, and the fine search has eight more 3-by-1
  moves (24 bearings) so it can cross a narrow wall at a shallower slant.
- **A graded bridge's line is straightened** (`Builder::shape`,
  `straighten`, `blend_onto`). The edge no longer holds its two nodes
  (which is what kept the dog-legs): the line runs straight through the
  crossing for half the span, the shelf and 25 m either side, turned toward
  the road's direction through the valley (at most 40 degrees off square to
  the channel and 45 to the water's flow); a cubic joins each end back to
  the line 250 to 60 m away (longest that fits), laid only if it keeps the
  class's radius, crosses no channel, keeps out of river water and blocked
  ground and stays within 4 m of the ground. Within 150 m of a bridge bends
  tighten to twice the class's least radius. A new check catches a smoothed
  stretch sliding into a river's water (a track ran through a creek's head
  at (10 231, -6 188) and `tests/roads.rs` caught it).
- **The record keeps the shelves dry** (`Builder::dry_shelf`,
  `src/roads.rs`). Abutments take the skew to the channel's indexed line
  and to the water's flow, whichever is skewer; a bridge's banks are read
  out by `(0.5 * width + bank_top) / square`; the shelf's corners, centre
  and back are walked and the abutment pushed out in 1 m steps (to 30 m)
  while any of them stands in a channel.
- Tried and dropped: earthwork carried along the path (loops and
  sawtooth switchbacks), and pricing a gorge creek as a rim-to-rim bridge
  in the search (more `Unplaced`, no showcase gain).
- Probes: `probe_alignment` (`grade_measure.rs`: turning and skew within
  150 m of each bridge, cut depths within 300 and 600 m) and `probe_skew`
  (`crossings_measure.rs`: what skew the kit tolerates, now naming each
  bridge with a channel under a shelf, each unplaced bridge and each with
  ground over the soffit's line).

### The review's findings, and what became of them

Five from the first review, all fixed:

- **Skew bridges put a shelf over the channel** (must-fix, confirmed): the
  record set abutments on the centreline only. Fixed by the skew rule and
  the shelf walk above. A flat push (corner width times the skew's
  tangent) was tried first and left two shelves over water and doubled the
  bridges with bank over the soffit, so it became the walk. The flow check
  was not added to the searched-edge fallback: that edge has the same
  direction, so it would change nothing; the walk protects any skew.
- **`straighten` overwrote held points** (confirmed): a straight now stops
  32 m short of any held point either side (`STRAIGHT_CLEAR_M`), the two
  halves may differ, and with no room for half the length the edge is held
  instead (by index). In the home mass 5 bridges' old ranges would have
  overwritten a held point.
- **`straighten` never checked the ground** (confirmed): each laid point
  off the deck gap stays within 4 m (`STRAIGHT_TOLERANCE_M`) of the ground
  it replaces, else a shorter share, the searched direction, then the hold.
  Of 640 graded bridges: 570 turned, 62 along the searched edge, 8 held.
- **The fallback could restore unchecked jogs** (confirmed): a straight is
  accepted only if both steps back onto the line cross no channel and run
  in no river water, so `raw_line` is valid by construction.
- **Earthwork was not discounted on an existing road** (confirmed): now it
  is, as travel is. (Not the cause of the nine extra roads.)

Four notes from the second review, left as notes except the wording:

- **The search prices the crossing against another deck height than the
  record** (culvert `surface + 1`, bridge `surface + freeboard`; the record
  `max(low bank, ...)` and a gorge promoted to a bridge at the rim). This
  is the likely cause of the showcase regression below. Not fixed; it is
  the first thing for the next pass.
- **The end test of an unheld bridge edge uses indices into `points`, not
  the searched ends**, so a searched end next to a stub is released.
  Not fixed (no measured effect).
- **The re-blessed fog hashes are verified in release only.** CI runs the
  `strata-weather` field test in debug, where it crashes on this machine
  before any test reports (SIGSEGV; once a UB-precondition abort inside
  `core`'s array iterator). It crashes the same with HEAD's `roads.rs`, so
  it predates this pass; per the machine rules it was not traced. Run it
  once in debug on a quiet machine before relying on CI.
- **Stale wording** ("sixteen moves", `MOVES`' doc): this section is the
  as-built record; the comments are for the next edit of that code.

### The measures (seed 42, release, home mass, before -> after)

Bake (`roads_measure probe_bake`, original and final binaries alternated,
14 CPUs under `taskset`; the documented 3.5 s is on 16 unpinned threads
and not comparable): before 4.41 / 4.32 / 4.29 / 4.44 / 4.45 s, after the
fixes 4.27 / 4.49 / 4.40 s (roads 2.38 to 2.61 s in all). About +1 %, noise.
Roads 5 258 -> 5 268; length 23 927 -> 24 021 km; unreached sites 14 -> 15,
none within 60 km.

Service-road bridges within 60 km (70 -> 71), `probe_alignment`:

- Turn over any 50 m within 150 m of the deck: median 45 -> 18 degrees,
  p90 90 -> 64. Bridges turning over 45: 41 -> 18; over 60: 26 -> 10; over
  80: 14 -> 6. Deck to the line 150 m out, median 45 -> 26 degrees. Skew:
  median 16, most 41 -> 45. Track bridges (7): essentially unchanged.
- Deepest cut within 300 m of a service bridge: median 6.3 -> 6.1 m, p90
  21.5 -> 17.4, most 26.4 -> 27.4; bridges over 8 m 21 -> 22.

Cutting within 60 km:

- Service length in cut over 3 m 5.7 % -> 5.3 %; over 8 m 2.89 % -> 2.33 %.
  Cuttings over 3 m 255 -> 243, over 12 m 42 -> 29, over 16 m 29 -> 18.
  Deepest 28.0 m at (-23 727, 26 018) -> 27.4 m at (52 180, 17 265).
  `probe_grade`: service in cut 76 % -> 73 %, over its grade 1.6 % -> 1.0 %.
- Tracks: over 8 m 5.79 % -> 5.10 %; deepest 23.3 -> 21.8 m; cuttings over
  16 m 45 -> 28; over grade 6.9 % -> 5.7 %. Highest track fill 15.9 ->
  31.5 m at (12 288, 45 374) (worse).

The showcase's first bridge, (7 579, -5 029), same place, still placed
(`bridge-service-20`, gap 24.3 m): turn over 50 m 90 -> 83 degrees, deck to
the line 150 m out 90 -> 117; deepest cut within 600 m 22.2 / 19.8 m (east /
west) -> 11.9 / 13.7 m.

Crossings within 200 km (`probe_placed`): 2 095 -> 2 068; unplaced graded
bridges 46 of 576 (8.0 %) -> 25 of 544 (4.6 %), of which `Unmet` 16 -> 8;
placed culverts with no headwall 92 -> 116.

Skew tolerance within 60 km (`probe_skew`): all 78 bridges within 45 degrees
of the flow, none with a channel under a shelf (the implementer's first
version had two, the shelf walk removed them); with 45 to 50 degree decks
allowed 2 of 4 had, so the cap stays. Placed bridges with ground over the
soffit's line 27 -> 14, worst 2.3 -> 3.4 m at 193/1 (24 342, -12 628): a
tight meander with water under one corner of the shelf and bank at the
other.

### Tests

Release, one binary at a time, `--test-threads=2`, after the fixes:

- `strata-worldgen`: lib 101 passed (5 ignored); `carve` 17; `coverage` 3;
  `continuity` 7; `determinism` 7 (1 ignored; 62 s); `drama` 2 (1 ignored);
  `geology` 13; `grade` 6; `port_serein` 5; `rainshadow` 4; `roads` 8;
  `sites` 6; `snapshots` 2; `version` 9; `perf` 3 ignored; every
  `*_measure` binary is all `#[ignore]` (probes, run by hand).
- `strata-weather`: 4 + 16 + 9 + 1 passed (1 ignored), release only.
- `strata-sim`: 62 passed (1 ignored). `strata-physics`: 13 + 9 + 1 (3
  ignored) + 16 + 1 passed. The physics and sim crossing tests that are
  ignored (release by hand, need `generated/`) were not run; the showcase
  bridge and the track ford they look up are at the same places.
- `cargo clippy -p strata-worldgen --all-targets -- -D warnings` clean;
  `rustfmt --edition 2024 --check` on every touched file clean; rustdoc of
  `strata-worldgen` without warnings (with `--document-private-items` two
  links in `carve.rs` and `strata.rs` fail, files not touched here).
- Not run: `just ci`, `just test`, `just bench`, `just server-check`;
  `just content` (nothing under `veyra-buildings` or `tools/content`
  changed, the kit is the same).

### What was blessed

Each after looking at why it moved:

- `GENERATOR_VERSION` 15 -> 16.
- `PINNED_CANARIES` (`hydrology/tests.rs`), all four: only roads moved
  (the no-sites canaries and `CANARIES_BEFORE_SITES` hold; the thread-count
  check passes). The last re-bless after the fixes moved the island canary
  alone, to `0x97b7_f4cc_8189_6aa8`.
- `world_version` `PINNED` (`tests/version.rs`) to `0x127e_bf98_ea54_59e0`.
- The `drama` `seed_42_measures` snapshot: slope classes within 0.1 %, a
  few profile heights, sandstone's riser share 11.5 -> 11.2 % from a moved
  road bank; the folded belt still 2.01. The `snapshots` `seed_42_samples`
  snapshot: the version line only.
- `strata-weather` `tests/field.rs`: the three bay-tile fog hashes
  (`0x4f41_30f4_1eba_2092`, `0x26e3_a811_591e_c7fe`, `0xfaf2_fb4f_080c_b499`),
  because roads' banks in that tile moved.
- Not moved: Port Serein's road 0 and the city's fingerprint
  (`port_serein`), the geology no-geology pin.

### Captures

`scratch/review/roads/`. `align-before-*` and `align-after-*` are worldmap
layers (`showcase`: 2 km round (7 550, -5 050); `mountain`: 8 km round
(12 300, -17 800)); `-roads` is the one to read. The game shots are release
under xvfb, `--fog 0 --time 0.45`; the renderer ran at 0.9 to 1.6 s a
frame here whatever the window size, so only four were taken and the last
three are weak:

- `align-01` (flycam 120 m over the bridge, yaw 1.87, pitch -0.55, 1500
  frames): the trench seen from above, the river winding on the left and
  the road a pale band curving down the right; the valley floor is in
  shadow and almost black, and the bridge deck itself cannot be told apart
  in it. It does not show the dog-leg either way.
- `align-02` (the player at (7 436, -5 078), 900 frames, 1280x720): did not
  land on the road; it stands on a wooded slope with the combat dummies
  and the river far below left. Shows nothing of the road. Needs a pose
  taken from the road's own points.
- `align-03` (flycam at (7 166, -5 304), yaw 2.36, 700 frames, 960x540):
  a wide plateau view with the sea behind and a thin grey road line
  crossing the middle left to right; the road reads as straight and the
  descent is not in the frame (the yaw is off the road's heading).
- `align-04` (the worldmap `-roads` layer round the bridge, same as
  `align-after-showcase-roads.png`): the backbone comes down the valley
  side from the west-north-west, still bends sharply onto the deck at the
  confluence and leaves it south-south-east, with the creek ravine's
  rim-height crossing just east: the remaining defect, plainly.

### What is left

- **The showcase's own crossing is not fixed.** The first bridge still
  turns 83 degrees within 50 m, and its approach cuts are 11.9 and 13.7 m
  against the 8 m aimed at. The river runs there in a carved trench about
  100 m wide and 50 m deep with 50 % walls through a plateau, at a
  confluence with a creek in its own ravine. The fine search has 64 m
  nodes and cannot lay a side-long descent along a wall narrower than a
  node or two; the straight and the blend cannot both fit between the
  confluence and the creek's held nodes. Turning the deck to 50 degrees off
  the flow gave 12 / 11 m cuts and was rejected (the channel under a
  shelf). It needs a finer local search on walls (about 16 m) or a higher,
  longer deck from terrace to terrace.
  *Round 3: grade and turn met, cuts not (see Round 3).*
- **A regression at the showcase**: the backbone now crosses the creek's
  ravine at rim height at (7 744, -5 087), about 160 m before the bridge:
  a 68.8 m gorge bridge with no class long enough (`NoClass`; the longest
  is 68.5 m), then a fall of about 30 m to the river deck in about 100 m.
  Before it was a low culvert reached through a 22 m cutting. The search
  prices that creek as a culvert dive while the record makes it a rim
  bridge (the review's first note): price both against the record's deck
  rule, or extend the span classes.
  *Round 2 fixed this; round 3 moved the bridge off the creek.*
- Seven service bridges still turn over 80 degrees within 50 m
  (confluences crossing several channels near (33 265, 33 223), tight
  Z-crossings of strongly meandering rivers, gorges).
  *Round 3: three.*
- The cutting is down but not gone: half of the deepest service cuts are
  not near a crossing and the 64 m search cannot shape them; heavier
  earthwork weights did not help.
- Side effects: culverts without a headwall 92 -> 116; highest track fill
  31.5 m; one more unreached site; the 193/1 meander above.
- The ticket's earlier "What is left" item on skew crossings is met in part
  (bridges may stand up to 45 degrees skew); culvert and ford skew limits
  are as before.
- The debug-build `strata-weather` field test crashing on this machine
  (above), and the stale comments about sixteen moves.

### Round 2 (2026-10-06)

The first review note was the cause of the showcase's regression and is
fixed; the showcase itself is still **not met**. `GENERATOR_VERSION`
stays 16. Not committed; targeted binaries only (release, two test
threads), as before.

**What changed** (`hydrology/roads.rs` unless said):

- **One rule for the record and the search** (`Builder::section`). The
  record's crossing logic (banks walked out to the rim, a gorge creek
  promoted to a bridge at the rim, the deck, the walks out to where the
  ground meets it) is now one function; the record calls it along the
  road's line, the search along the edge (graded classes only: a trail
  has no deck and no earthwork, and is priced as before). The search
  prices the crossing by its kind and deck, a bridge by its gap as read
  (so a skew or a gorge deck is dearer by what it really spans), and
  **closes** an edge whose bridge has a gap over
  `crossings::BRIDGE_GAP_MOST_M` (68.5 m) or a deck too far over the bed
  for `crossings::PIER_MOST_M` (48 m) on a span with piers (gap over
  `PIERLESS_GAP_MOST_M`, 28.5 m). The three limits are the kit's;
  `veyra-buildings`' `the_world_knows_the_kit_limits` holds the
  catalogue to them. The walks share their points (a memo, the out walk
  on the rim walk's steps) and skip the water's own width.
- **The search reads a crossing only when it could matter**: an edge is
  first bounded below (`Class::earthwork_least`, the travel, the least the
  kind can cost) and dropped if that cannot better its node, else queued
  at that bound and read off the ground only when it comes up (lazy edge
  evaluation; the result is the same path, ties apart). Without this the
  rule cost 1.1 s of a 4.3 s bake.
- **A service road's corridor is 1.6 times as wide where its lattice
  route crosses a river a culvert does not take** (`RIVER_WIDEN`). With the
  closing alone the showcase's backbone, kept south of the creek by its
  corridor, dived into the creek's ravine for a culvert and climbed out at
  49 %; the search in a wider corridor found the creek's north rim 19 %
  cheaper. Tracks are not widened (their measures did not gain).
- Stale "sixteen moves" comments fixed. `probe_alignment` prints the
  showcase's steepest grade and the crossings near it; `probe_nodes`
  (`grade_measure.rs`) prints the 64 m nodes' ground round a point.

**Why the showcase still crosses at the confluence.** Measured by
re-searching the backbone's route (a debug harness, removed): the search's
path costs 24 418; with crossings within 250 m of the confluence banned it
crosses upstream at about (7 264, -4 448) for 28 224; with an unweighted
heuristic (pull 1) in the same corridor it keeps the confluence (23 608);
with pull 1 in a corridor 2.2 times wider it crosses upstream for 23 380.
So the upstream crossing does not lose on a cost term: the two are within
1 % under the cost model, and the weighted search (pull 1.6) in the
lattice's corridor settles on the confluence. Both need the same thing:
the trench is 250 m rim to rim (no class spans it) and 50 m deep, with
walls one or two 64 m nodes wide, so every crossing comes down a wall in
one or two edges. The edge-by-edge earthwork prices two consecutive steep
edges as the sum of their squares, less than the cutting the profile then
needs between them and the deck; carrying it along the path was tried in
round 1 and dropped. The fix is still a finer search (about 16 m) on the
walls near a crossing; not done.

**Measures** (seed 42, release, home mass; round 1 -> round 2):

- Showcase's first bridge (7 579, -5 029), same bridge
  (`bridge-service-20`, gap 24.3 m): turn within 50 m 83 -> 72 degrees;
  deck to the line 150 m out 117 -> 72; deepest cut within 600 m east
  11.9 -> 25.3 m (at (7 691, -5 029)), west 13.7 -> 10.0 m; steepest grade
  within 300 m east 43.7 -> 44.5 %, west 8.0 -> 8.0 %. The creek: round 1
  a 74.8 m rim bridge at (7 686, -5 075) that stood nothing (`NoClass`)
  and a 30 m fall to the deck; now no creek crossing (the road keeps the
  ravine's north rim and comes down the trench's wall to the deck; round
  1's east cut was measured beside that gap, which the probe skips).
  `scratch/review/roads/align2-showcase-roads.png`.
- Within 60 km, `probe_alignment`: service bridges 71 -> 68; turning over
  45 degrees within 50 m 18 -> 18, over 80 6 -> 6 (median 18 -> 23); 150 m
  out median 41 -> 40; skew most 45 -> 46 (one bridge); approach cut p90
  17.4 -> 19.4 m, over 8 m 22 -> 20. Service length in cut over 8 m 2.33 ->
  2.31 %; cuttings over 12 m 29 -> 25, over 16 m 18 -> 19. Tracks: bridges
  7 -> 4, over 8 m 5.10 -> 5.23 %, cuttings over 16 m 28 -> 31.
- `probe_grade` (60 km): service over its grade 1.0 -> 1.1 %, cut over
  10 m 10.6 -> 10.5 km; tracks over grade 5.7 -> 5.8 %, over 10 m 37.3 ->
  38.3 km. About level: the bridges that stood nothing were measured as
  level gaps in round 1, and those roads now come down instead.
- Within 200 km, `probe_placed`: graded bridges 544 -> 491; unplaced 25
  (4.6 %) -> 9 (1.8 %): `NoClass` 15 -> 3, `TooTall` 2 -> 1, `Unmet` 8 -> 5.
  Culverts with no headwall 116 -> 124.
- `probe_skew` (60 km): 78 bridges, 74 placed -> 72, all placed; none
  with the channel under a shelf; ground over the soffit's line 14 -> 9,
  worst 3.4 -> 2.0 m (the 46 degree one, (-46 496, -12 832)).
- Roads 5 268 -> 5 272, 24 021 -> 24 023 km; unreached sites 15 -> 15.
- Bake (`probe_bake`, five alternating pairs, 14 CPUs): round 1 4.31 /
  4.46 / 4.28 / 4.40 / 4.47 s (roads 2.40 to 2.56), round 2 4.54 / 4.64 /
  4.63 / 4.71 / 4.66 s (roads 2.65 to 2.77): **+0.25 s (+6 %), not within
  noise**. About half is the crossing reads, half the wider corridor
  (`RIVER_WIDEN` 1.0 takes it back, and with it the showcase's creek dive
  returns and service cut over 8 m goes to 2.67 %).

**Tests** (release, one binary at a time, two threads): `strata-worldgen`
lib 101 (5 ignored), `roads` 8, `grade` 6, `port_serein` 5, `version` 9,
`determinism` 7 (1 ignored), `snapshots` 2, `drama` 2 (1 ignored), `carve`
17; `strata-weather` 4 + 16 + 9 + 1 (release); `veyra-buildings`
`bridge::` 4 (debug). `cargo clippy -p strata-worldgen --all-targets -D
warnings` and `-p veyra-buildings` clean; `rustfmt --edition 2024` on the
touched files; rustdoc of `strata-worldgen` with warnings fatal clean.
Not run: `strata-sim`, `strata-physics` (their ignored crossing tests
look up the showcase bridge, which stands where it stood).

**Blessed**, each after checking that only the roads moved:
`PINNED_CANARIES` (three of four: the island `0x48eb_da47_c489_23cb`, the
third and fourth; `CANARIES_BEFORE_SITES` and the thread-count checks
hold), `world_version` to `0x60a3_f938_2150_eb36` (the canary networks'
roads), the `drama` snapshot (slope medians by 0.1 degree, badlands' and
sandstone's riser shares by 0.03 and 0.01: road banks). Not moved: the
`snapshots` samples, Port Serein's road 0 and fingerprint, the
`strata-weather` bay-tile fog hashes.

**Left:**

- The showcase: the east approach comes down the trench's wall in one
  edge (25 m cut, 44.5 % on the profile); the Z at the deck is 72 degrees.
  Needs the finer wall search above, or a less greedy search near
  crossings (time).
- The bake is 6 % slower.
- Nine graded bridges within 200 km still stand nothing, where the shaped
  line reads the crossing otherwise than the searched edge (the record's
  walk follows the final line): `NoClass` on long floodplain gaps at
  (138 208, 69 920) and (49 059, 132 707), and an `Unmet` gorge creek at
  (89 718, 35 126) whose rim walk climbed a mountainside to a deck 291 m
  over the bed (a pierless gap, which the pier limit does not bound).
- One bridge at 46 degrees to the flow (the search tests skew against the
  channel's indexed line, the record against the flow too).
- Tracks lost three bridges to fords and gained a little cutting; culverts
  with no headwall 116 -> 124.
- The second review's note on the unheld bridge edge's end test, and the
  debug-build `strata-weather` crash, as before.

**Captures** (finish stage, release game, `--flycam --fog 0`, showcase bridge
at (7 579, -5 029), the road crossing it towards the north-east):

- `scratch/review/roads/align2-01.png`: 120 m up, from just south-west of
  the bridge, looking along the road's travel. The bridge, its kink on the
  west bank and the road climbing the east wall in a deep cut; the cut and
  the turn at the deck are still visible.
- `scratch/review/roads/align2-02.png`: on the road 150 m before the bridge,
  2 m up, looking at it. A clean graded road in a shallow cut; the bridge
  itself is hidden by the dip and a rock outcrop.
- `scratch/review/roads/align2-03.png`: the valley from 60 m up, side on from
  the south. The deck over the river and the road coming down the trench
  wall at the right; the descent reads steep.
- `scratch/review/roads/align2-04.png`: the worldmap `-roads` layer of 3 km
  round the bridge: the backbone crosses at the confluence, with the kink
  at the deck and no creek bridge.

### Round 3 (2026-10-09)

A finer search on the valley walls near a crossing, as round 2 named it.
The showcase is **partly met**: the east approach's grade and the turn at
the deck meet their marks, the cut depths do not. `GENERATOR_VERSION` 16
-> 17. Not committed; targeted binaries only (release, two test threads).

**What changed** (new `hydrology/roads/walls.rs`, 1 400 lines with its
tests' share; `hydrology/roads.rs` +320 -75):

- **Windows.** The 64 m search and the per-sample grade are unchanged
  (`earthwork` and `earthwork_least` give the same numbers). In `finish`
  each graded road's searched line is built first (`Builder::searched`).
  Round each bridge over water at most `WIDTH_MOST_M` (30 m) wide whose
  searched edges within `REACH_M` (512 m) carry over `STEEP_EARTH`
  (100 m2) of earthwork (130 of 324 bridges on the home mass), a window is
  planned out to the first node at least 512 m along the line each way,
  short of other crossings. Windows are separate parallel work items (one
  long road holding several had pinned a thread).
- **The search.** Decks are tried at the searched crossing and slid
  `SLIDES` (96 and 192 m, up and down the river), each square and at the
  `TURNS` angles (20 and 40 degrees off it). Each is read by
  `Builder::section` and the kit's limits (`kit_stands`), then held
  straight 25 m past its shelves. From each straight end an A* on 16 m
  nodes (`WALL_M`, 24 bearings) runs to that side's window end with the
  fine search's travel, side slope, belt, turn and hairpin costs, through
  `Class::step` and `Class::turn_cost`, now shared by the fine search,
  `straighten` and this. It may not cross a channel, bends no tighter than
  `BEND` (0.8) of the class radius (twice that within 150 m
  of the deck), never turns past a right angle in one step, and runs at
  most `LONGEST` (1.75) times the searched line's own length to that end.
  Earthwork is carried as a profile: it leaves the deck at deck level,
  follows the ground as closely as the grade allows and pays the depth
  left (less 2 m free) times length. The cheapest deck with its two ways
  replaces the window, only where it beats the searched line priced the
  same way (`Builder::searched_cost`). Shaping then runs as before, with a
  wall-laid bridge's straight (`straight_m`) kept on its own bearing.
- **Junctions and other roads.** A window never moves a node another road
  ends on: its ways run through those nodes as waypoints, one leg each
  (`Builder::leg`). Nodes within `ROAD_CLEAR_M` (24 m) of another road's
  searched line (or its own road's outside the window) are closed, and
  moves near a line (`ROAD_TEST_M`, 32 m) are tested for crossing it
  (`Builder::lines`, a `LINE_BUCKET_M` index).
- Heuristic scaled to be admissible; turn cost scaled by step length so a
  bend costs the same per metre at 16 and 64 m; a per-window `Labels` set
  reset by a generation stamp.
- `probe_alignment`, `probe_nodes` (`AT_STEP`), `probe_skew` and
  `probe_placed` now look at the showcase where it stands (7 404, -4 940).
  The two ignored showcase tests in `strata-physics/tests/crossings.rs` use
  a `SHOWCASE` const for it.

**The review's findings, and what became of them.**

- *Windows ignored other roads' junctions* (must fix). The first build
  moved 85 junctions over 5 m, 37 over 50 m, 16 over 100 m, worst 264 m;
  track 997 crossed road 54 without a junction, road 306 acquired a 51 m
  bridge nobody searched. Fixed as above (waypoints and the keep-off).
  Ending windows short of a junction was tried first and put the showcase
  back to round 2, a trail junction 170 m east of the crossing cutting the
  window off. Now of 8 397 junctions 31 move over 5 m, 7 over 20 m, none
  over 50 m, worst 45 m; near-bridge junction turns worst 65 degrees,
  none over 90, as round 2; proper road crossings 1, as round 2.
- *The new Unmet bridge was misattributed* (must fix). It was road 306's
  eased end, not a skew misreading; gone with the fix above. Graded
  bridges standing nothing are 4 (NoClass 1, TooTall 1, Unmet 2), none a
  wall deck.
- *The wall result replaced the searched line unconditionally*: now kept
  only on a strict improvement, and the searched price seeds the pruning.
- *Inadmissible heuristic*: scaled by the least a move can cost.
- *Turn costs differ across resolutions*: scaled; a reversal within 64 m
  pays the hairpin, which the 16 m steps had made unreachable.
- *`LONGEST` against the wrong length*: now the searched line's own arc.
- *Duplicated move cost and skew test*: shared helpers in `roads.rs`; the
  fine search's results are unchanged.
- *Per-call buffers*: one set per window.
- *The straight not held in shaping*: it is now (the heading over the
  deck holds from -25 to +25 m; it swung 14 to 22 degrees).
- *Found while measuring*: `BEND` 0.6 shaped a 30 m switchback into a U
  20 m across (a 180 degree turn within 50 m at (12 478, -6 963)); 0.8
  removes it, 1.0 gives a west cut of 20.5 m.
- Declined: one label per (node, heading) (about twenty times the work
  for a +5 % bake already near its budget; `Builder::side` is documented
  as a heuristic); holding junction points fixed in the shaping (tried,
  junction moves unchanged, approach cut p90 17.2 -> 18.4 m).

**Measures** (seed 42, release, home mass; round 2 -> round 3):

- Showcase's first bridge, which moved 190 m up the river to (7 404,
  -4 940) (now `bridge-service-28` with a pier, gap 24.3 -> 32.2 m, deck
  67.7 m, skew 44 degrees, placed): turn within 50 m **72 -> 38 degrees**;
  steepest grade within 300 m east **44.5 -> 8.0 %**, west 8.0 -> 8.0 %;
  deepest cut within 600 m east **25.3 -> 17.8 m** (at (7 672, -5 040)),
  west **10.0 -> 16.0 m** (at (7 166, -4 787)); the road 15 269 -> 15 248 m.
  The cut marks (east under 12 m, west no worse than 10 m) are **not
  met**. Both ways must pass through junctions the fine search put high on
  the trench's walls, a trail at (7 744, -5 056) (ground 108 m) and a road
  at (7 168, -4 800) (ground 107 m), each about 40 m above the 67.7 m deck;
  at 8 % that is 500 m of road, and the cost model finds no line in the
  window that beats the cut. The track that ends at the west junction
  inherits the backbone's level there: its 22.0 m cut at (7 162, -4 804) is
  the deepest track cutting within 60 km.
- Within 60 km, `probe_alignment`: service bridges 68 -> 68; turning over
  45 degrees within 50 m 18 -> 14, over 60 9 -> 7 (round 2 10), over 80 6
  -> 3, median 23 -> 22; skew most 46 -> 46 degrees, over 45 1 -> 1;
  approach cut p90 19.4 -> 17.2 m, over 8 m 20 -> 17, median 5.7 -> 5.6.
  Service length in cut over 8 m 2.31 -> 2.06 %; cuttings over 12 m 25 ->
  24, over 16 m 19 -> 17. Tracks: over 8 m 5.23 -> 5.24 %, cuttings over
  16 m 31 -> 32.
- Within 200 km, `probe_placed`: graded bridges standing nothing 9 -> 4
  (`NoClass` 3 -> 1, `TooTall` 1 -> 1, `Unmet` 5 -> 2); service bridges
  placed 412 -> 417; service culverts with no headwall 69 -> 69.
- `probe_skew` (60 km): 72 bridges, all placed, one over 45 degrees (round
  2's (-46 496, -12 832), 46 degrees), none with the channel under a shelf.
- Roads 5 272 -> 5 272, 24 023 -> 24 033 km; unreached sites 15 -> 15.
- Bake (`probe_bake`): the builder's five alternating pairs round 2 4.981 s
  (roads 2.798) -> 5.230 s (roads 3.068), **+0.25 s (+5 %)**; the
  verifier's single run 5.363 s (roads 3.028 s) on a slower machine day,
  +7.6 % against the same pairs' mean: within the +10 % budget, not
  within noise. The wall search costs 2 to 3 s of CPU a bake, in parallel
  per window.

**Tests** (release, one binary at a time, two threads; verifier's run):
`strata-worldgen` lib 101 (5 ignored), `roads` 8, `grade` 6,
`port_serein` 5, `version` 9, `determinism` 7 (1 ignored), `snapshots` 2,
`drama` 2 (1 ignored), `carve` 17, `continuity` 7, `geology` 13;
`strata-weather` 16 + 4 + 9 + 1; `strata-sim` 62 (1 ignored);
`strata-physics` all binaries ok. The builder ran the two ignored
showcase-bridge tests in `strata-physics` (walk and run across the deck,
the rails) once `generated/` was present: both pass. `cargo clippy -p
strata-worldgen --all-targets -D warnings` clean, `rustfmt --edition 2024
--check` clean on the touched files and `roads/walls.rs`, rustdoc with
warnings fatal clean. Not run: `just ci`, the whole workspace, game
captures.

**Blessed**, each after checking that only the roads moved:
`GENERATOR_VERSION` 17; `PINNED_CANARIES` the island only, `0x48eb_da47_c489_23cb`
-> `0xc5ae_4916_d5c4_024a` (the other three, `CANARIES_BEFORE_SITES` and the
thread-count checks hold); `world_version` `0x60a3_f938_2150_eb36` ->
`0x0a24_b0e8_ffa4_563b` (the version bump and the island's roads); the
`snapshots` samples: the version line only (16 -> 17), no sample moved; the
`drama` snapshot: the 2 to 5 degree band 26.8 -> 26.9 %, coast mean slope
10.5 -> 10.4, badlands' riser share 11.14 -> 11.12, sandstone's 5.7 -> 5.6 %
and 11.2 -> 11.3 %, road banks only. Not moved: Port Serein's road 0 and
fingerprint, the `strata-weather` fog hashes.

**Left:**

- The showcase's cuts: east 17.8 m (aimed under 12), west 16.0 m (was
  10.0). It needs the junctions on the trench walls to sit lower (the fine
  search places them), or the window to be free to move them. Round 3's
  first build met more of the cut marks (east 19.0, west 18.0, turn 45
  degrees) only by moving 85 junctions up to 264 m; that was the defect
  the review found. Choosing between this and holding the deck at the
  confluence (met the east marks at 8.0 %, 10.7 m, 38 degrees, but looped
  the west side 1.6 km with a 20.8 m cut) needs a look in the game; no
  game captures were taken of the new line.
- The carried profile looks forward only and a node keeps one label, so a
  way that runs flat on a terrace and then climbs a wall is underpriced.
- The bake is 5 to 8 % slower than round 2's (11 to 14 % over round 1's).
- Four graded bridges within 200 km stand nothing: `NoClass` at
  (138 208, 69 920), `TooTall` at (68 937, 182 619), `Unmet` at (170 273,
  -95 874) and (89 718, 35 126).
- One bridge at 46 degrees to the flow; three service bridges still turn
  over 80 degrees within 50 m.
- The track ending at the showcase's west junction has the deepest track
  cutting within 60 km, 22.0 m.
- The showcase moved: `--flycam --spawn 7404,-4940 --look 1.47` looks
  along the deck toward -x; the earlier entries' (7 579, -5 029) is where
  round 2's bridge stood. The strata-sim crossings test still searches 5
  km round the old point, which covers it.
- The second review's note on the unheld bridge edge's end test, and the
  debug-build `strata-weather` crash, as before.

**Captures** (release game, `--flycam --fog 0`, xvfb, 240 frames, at the
bridge's new place (7 404, -4 940); the poses are round 2's, moved with the
bridge; the road crosses west to east here):

- `scratch/review/roads/align3-01.png`: 120 m up from just south-west of
  the bridge (`--spawn 7300,-4890 --look -1.3`), looking along the road's
  travel. The road comes in from the near left beside the river, crosses on
  a short straight deck with its pier and goes on beside the river in a
  narrow shelf, with no kink at the deck. Against `align2-01.png`, where the
  road made a hard turn at the deck and climbed the east wall in a deep cut
  with the creek's fork under it, the crossing is now a single gentle line
  and no deep gash shows on either side. The bridge stands 190 m up the
  river, so the confluence is no longer in the frame.
- `scratch/review/roads/align3-02.png`: on the road 150 m west of the deck,
  2 m up (`--spawn 7254,-4940 --look -1.57 --camera-altitude 2`). The road
  runs along the trench wall as a graded shelf at a steady fall, a cut bank
  above it and the river below; the deck is small and far ahead. The wall
  is traversed side-long, not descended edge by edge, which is what the
  window search set out to do. The west cut (16.0 m measured) is not
  readable from this height.
- `scratch/review/roads/align3-03.png`: the valley from 60 m up, from the
  south (`--spawn 7404,-4700 --camera-altitude 60`). The deck is square to
  the river; the east road leaves it over the plateau rim in a long even
  line, with no steep face. On the west the road makes a long diagonal down
  to the river from a switch at the far left, and its dark cut bank is the
  most visible earthwork in the frame, which agrees with the measured 16.0
  m west cut being worse than round 2's 10.0 m.
- `scratch/review/roads/align3-04.png`: the worldmap `-roads` layer of 3 km
  round the bridge. The backbone runs down to the trench, follows it on the
  rim side for a few hundred metres, crosses once at the pier bridge (`BR`)
  and leaves east; the creek's fork has no bridge, and a trail leaves the
  junction beside it. No hairpin shows near the deck.

Read for the success marks: the east approach now meets the deck at grade
along the wall (8 % measured) and the turn is gone; the cuts are shallower
than round 2's east, but both approaches still cut, the west one visibly.

## Step 8a, as built (2026-10-09)

Rivers and roads painted at distance (ticket 0004's step 10): the far
terrain draws the network's own lines, where the water mesh and the splat
stop. Built, reviewed (verdict fit, no must-fix) and the review's findings
fixed the same day. No sample, network or worldgen behaviour changed;
`GENERATOR_VERSION` is still 17 and nothing was blessed. Not committed;
`just ci`, `just test` and `just bench` not run (targeted binaries only,
release, two test threads).

### What was built

- **The lines, not the vertex record** (`strata_terrain::paint`). A far
  region's vertices are 64 to 256 m apart and a river or a road 2 to 30 m
  wide, so a share per vertex misses the line or smears it, and the
  shader's sixteen locations are all used. Each region's meshing task
  gathers the lines that reach it (`River::centreline`, finely enough for
  its meanders, and `Road::points` split at its crossings), simplifies
  them to a thirty-second of the level's spacing, and packs them, widest
  first, into a fixed 17 424-byte slot of a third store buffer beside a 16
  by 16 grid of buckets. A region with nothing to paint writes one word.
- **The shader** (`terrain.wgsl`, `paint_at`) reads its region's bucket by
  its `MeshTag`, measures its distance to each stroke there and covers the
  stroke's real width, antialiased by the pixel's footprint across it. The
  line is the same at every level, so nothing moves where two meet.
- **Rivers** hand over to the water mesh by the mesh's own rule
  (`RESOLVED_CELLS`, the parent level, the morph band) turned round; lakes
  are left to the mesh, which draws them at every level.
- **Roads.** A level whose spacing is a carriageway's width or more (LOD 3
  up for service roads and tracks, LOD 2 up for trails) leaves the roads
  its paint holds out of its layer shares, rock and tint, and the paint
  draws them in their class's material, lifted with distance. Below that
  the splat hands over across the morph band. Which samples are a road's
  is the painted line's to say (`PaintedWays`, handed to
  `build_region_mesh_painted`), not their material's, which a road shares
  with a gravel yard, a settlement's road and a channel's bed; a line the
  slot left out keeps its splat. An authored settlement's own road is its
  laid ground and is not painted.
- **Crossings.** A bridge's gap is painted only past `DECK_DRAWN_M` (4 km,
  the game's `crossings::DRAW_M`), faded by the same horizontal distance
  the deck is spawned by; a ford's way under the water, a culvert's road
  over its creek.
- **Off switch.** `--no-far-paint` (`TerrainConfig::paint`) gathers no
  paint and compiles the shader without it (`TERRAIN_FAR_PAINT`); the
  coarse levels then keep every road in their splat.

### Where it left the design, and why

The ticket asked for rivers and roads in the terrain's look at distance
(0004, step 10). The design left the vertex record and the world texture
for vector strokes in a slot per region, because both of those lose a line
two to thirty metres wide on a grid 64 to 256 m apart, and the shader has
no free varying. It left the sample, the network and the generator alone:
`GENERATOR_VERSION` is 17 and no pin moved. It changed the mid-distance
road look on purpose, since a level a carriageway's width or wider no
longer draws that road in its splat. The one change outside the terrain is
`WAY_MATERIALS` made `pub` in worldgen, a visibility change, and the
game's `crossings::DRAW_M` now names the paint's `DECK_DRAWN_M`.

### The review's findings, and what became of them

- **Port Serein's road drawn twice in two materials** at distance (the
  settlement's gravel splat and a setts stroke). Fixed: the settlement's
  own road is not painted (`road.is_settlements()`), so it is the
  settlement's laid ground at every level as before step 8a.
- **The bridge gap faded by the 3D distance** while the deck is spawned by
  the horizontal one, so from altitude the road showed under a drawn deck.
  Fixed: the fade reads the horizontal distance from
  `view.lod_view_world_position`, as the morph does.
- **Meanders drawn as a sawtooth.** Douglas and Peucker at an eighth of the
  spacing (about 1.4 px at a region's nearest, 160 spacings away, not the
  "few tenths" the docs said) kept only the meander peaks. Fixed: the
  tolerance is a thirty-second of the spacing (about a third of a pixel),
  the docs of `tolerance_m` and `narrowest_m` corrected, and the slot
  doubled (9 232 to 17 424 bytes) so the denser lines fit: at the old size
  the finer tolerance had to widen in 9 of the showcase's 36 level-6
  regions, now in 3. The meander in capture 02 is rounded
  now where it had sharp corners; its period is the channel's own (an 8 m
  reach swinging 15 to 20 m every 100 m or so).
- **The shader's paint constants unchecked.** Fixed:
  `the_paint_layout_matches_the_packer` (tests/shaders.rs) holds the slot's
  words, grid, header, entry word, stroke words, the bucket bound, the
  kind and flag bits, the deck's range and fade and `RESOLVED_CELLS` to
  the crate's.
- **The showcase test's last assertion tested nothing.** Replaced: on a 4
  m grid 600 m square round the bridge, every sample in its channel (by
  `Sample::water`, a metre and the tolerance inside the edge) is painted
  water (227 of 227) and no sample outside it past the tolerance and half
  a pixel is (0 of 22 073).
- **`PAINT_MOST` capped a bucket silently.** Fixed: the fit keeps every
  bucket to `BUCKET_MOST` (48, the shader's bound) and leaves out a line
  rather than let the shader drop strokes, the fullest bucket is counted
  (`RegionPaint::fullest`, `TerrainStats::paint_fullest`, the far paint
  log line, the timing) and a test holds the two bounds equal. The
  buckets list a stroke only where it can cover their square
  (segment-to-box distance, not the centre and half a diagonal).
- **The mesher's dropped-way rule keyed on material.** Fixed as above
  (`PaintedWays`): a gravel yard, a settlement's road or a channel's bed
  under a road's cleared verge stays laid; the mesh test covers the yard.
- **`WAY_MATERIALS` copied.** Fixed: `strata_worldgen::procedural`'s is
  public (visibility only) and the terrain reads it.
- **Every fragment pays for the paint.** Measured, same session, river
  flight: opaque pass 7.29 ms with the paint (three runs, 7.10 to 7.49),
  7.15 ms compiled out (four runs, 7.01 to 7.34), 7.18 ms with the paint's
  code removed by hand; the run-to-run spread is 0.2 to 0.4 ms. The stroke
  loop is under the noise; most of the 0.15 ms is the code's presence,
  which a skip by level or distance would not remove. So the paint is
  compiled out when it is off, rather than skipped near.
- **Docs.** This section, and one sentence in CLAUDE.md after the river
  water's, added by the convention sweep of 2026-10-10 (see "What is left").

### The measures (seed 42, release)

- `paint_time_per_region` (sky_timing, ignored), 36 regions a level round
  the bridge, in the game's order (paint, then the mesh with the painted
  ways): paint 0.02 to 0.13 ms a region at LOD 0 to 6, 0.22 at LOD 7, 1.4
  at LOD 8, 1.0 % of the mesh's 196 ms summed; the mesh time is unchanged
  (196.1 ms against 196.4 before the fixes). Strokes a region 4.0 at LOD 0,
  134.6 at LOD 6 (most 513); 0.26 to 1.27 strokes a bucket on average;
  fullest bucket 45 at LOD 6, 48 at LOD 8; widened 3, 6 and 9 of 36 at LOD
  6, 7 and 8; no line left out.
- The river flight (`--flight --flight-heading -1.5708 --spawn -2000,-4940
  --fog 0 --frames 1000`, xvfb, first five windows skipped): 11.8 strokes
  and 48 us a region over 6 300 regions, none left out; upload 24.1 MB/s
  and pool 1.25 thread-s/s with the paint against 24.4 MB/s and 1.27 off.
  GPU times as in the review's last finding; the xvfb frame rate is paced
  at about 16 fps and says nothing.

### Tests

strata-terrain lib 86 (the paint's eight, among them the box distance and
the channel test, and `a_coarse_level_leaves_a_carriageway_to_the_paint`),
shaders 6 (both variants of `terrain.wgsl` compose and validate, the paint
buffer bound only with the def), headless 2, made_ground (ignored);
strata-worldgen lib 101, port_serein 5, version 9, snapshots 2,
determinism 7, all unchanged. Clippy (`-D warnings`) on strata-terrain,
strata-worldgen and game; rustdoc on strata-terrain with warnings fatal.

Run after the fixes, one binary at a time, two test threads: `rustfmt
--check` clean on every touched file; clippy with warnings fatal clean for
`strata-terrain`, `strata-worldgen` and `game`; rustdoc with warnings
fatal clean for the first two; `strata-terrain` release 86 lib, 2
headless, 6 shaders; `strata-worldgen` lib 101, `port_serein` 5,
`version` 9, `snapshots` 2, `determinism` 7, all passing; the game's
headless run of ten frames and `cargo check -p server` clean. The diff
carries no pin, snapshot or `GENERATOR_VERSION` change and none of
`Cargo.toml`, `Cargo.lock`, `README.md` or `justfile` belongs to it.

### Captures

`scratch/review/far/` (release game, `--flycam --lock-camera --fog 0`,
xvfb):

- `01-bridge-120m.png`: the showcase bridge from 120 m (`--spawn
  7170,-4875 --camera-altitude 120 --look -1.3 --camera-pitch -0.36
  --frames 600`). The water mesh near, the painted river and road on to
  the horizon, no step where the mesh ends.
- `02-river-country-1500m.png`: 1 500 m over the bridge looking north-east
  (`--spawn 7404,-4940 --camera-altitude 1500 --look -0.785
  --camera-pitch -0.2 --frames 900`). `03-river-country-no-rivers.png` is
  the same with `--no-rivers`; `02-before-paint.png` the baseline before
  step 8a.
- `04-worldmap-rivers-roads.png`: the worldmap's `-roads` layer round the
  showcase, unchanged.
- `05-join-300m.png` and `06-join-300m-no-rivers.png`: 300 m up looking up
  the valley (`--spawn 7170,-4875 --camera-altitude 300 --look -1.3
  --frames 700`), with and without the water mesh.
- `07-port-serein-road-3km.png` and `07b-port-serein-road-3km-no-paint.png`:
  Port Serein's road from 2.5 to 3.7 km, 600 m up past its head looking
  back at the city (`--spawn=-2671,-3629 --camera-altitude 600
  --look=-2.52 --frames 700`), with and without `--no-far-paint`. The
  settlement's road is the same pale gravel in both; the network's road
  from its head is the paint's line in one and the splat's dashes in the
  other.
- `showcase.png`: the first look, 120 m up 212 m south-west of the
  bridge looking north-east at a steep pitch (`--spawn=7254,-5090
  --camera-altitude 120 --look=-2.356 --camera-pitch=-0.6 --frames 700`,
  `--flycam --lock-camera --fog 0`). The deck, the road crossing it and
  the meandering channel all sit on the near mesh; the paint is not yet in
  reach at this height, so it shows the mesh alone.
- Judged 2026-10-10 from all ten stills: every pose in the brief was
  present and shows the place asked for; none was retaken. The river and
  road read at 1 500 m (02 against 02-before-paint) and 03 matches 02; 05
  and 06 show the join with a faint tonal step; the far river is a thin,
  slightly jagged dark line.

### What is left

- **Look calls for the owner.** The far road's lift (up to 1.5 times its
  material's mean between 1 and 5 km) reads slightly bright at 2 to 5 km;
  the far water's colour and roughness are shader constants; at grazing
  angles past 10 to 15 km tracks and trails fade, being covered by their
  true share of a pixel; Port Serein's own road stays the settlement's
  gravel blur at distance (it could be painted in its own material and
  left out of the splat instead).
- **The deck's ends.** The paint stops a road at the crossing's
  `abutments_m`, not at the deck's ends (`DeckEnd`, `length_m`, the chord):
  the terrain crate has no crossing kit. Within 4 km the deck covers the
  overlap. A receding observer keeps a deck to 4.4 km (`DROP_M`) while the
  paint fills its gap from 4 km: the two overlap in that band, and no
  single fade satisfies both directions.
- **Pinned regions.** A detail zone's region never fades its creek water,
  but the paint's `mesh_share` ignores pinning, so in a pinned region's
  morph band the dark paint shows under the still-drawn clear water.
- **Lakes** are not painted (the mesh draws them at every level); a
  reach's line inside a lake is painted under its water. **Falls** are
  painted as plain water; their sheet and spray are ticket 0004's step 11.
- With `--no-rivers` the near channel shows its gravel bed: the paint
  defers to the mesh's rule whether or not the mesh is built.
- The finer tolerance doubles the strokes and widens the tolerance in some
  dense far regions (above); the bucket bound is met at LOD 8, where the
  fit widens rather than drop a line.
- **CLAUDE.md** has its sentence after the river water's, added by the
  convention sweep of 2026-10-10; the owner may reword it.

## Step 8b, as built (2026-10-09)

Falls (ticket 0004's step 11): a fall gets a sheet, foam and spray of its
own over the river's ramp. Built, reviewed (verdict needs-fixes, one
must-fix) and the review's findings fixed. No sample, network or worldgen
behaviour changed; `GENERATOR_VERSION` is still 17 and nothing was
blessed. Not committed; `just ci`, `just test`, `just server-check` and
`just bench` not run (targeted binaries only, release, two test threads).

### What was built

- **A mesh per fall** (`strata_terrain::falls`). `build_fall_mesh` is a
  pure function of the `Fall` record and `Network::water_at`, so it needs
  no sample and no bake. The sheet is a ribbon from the lip to the foot,
  a little wider than the channel, that follows the arc water takes off
  the lip, stands 0.3 to 1.6 m off the face and bulges across; its
  coordinate down the fall is travel time, so the streaks speed up as the
  water falls. Foam lies on the network's water level upstream of the lip
  and round the foot. Spray is camera-facing puffs in a second entity
  whose material writes no depth, its whole cycle in the vertex shader.
  About 350 vertices a fall.
- **Streaming** (`stream_falls`, in `WaterPlugin`). It reads only
  networks already baked, rescans every 250 m or when a network is baked,
  builds 6 falls a frame nearest first, draws to 18 km with a fade over
  the last 2 km and drops them past 18.8 km. A fall is drawn only when
  its drop is 6 m or more and its slope 0.12 or more (`is_drawn`).
- **The shader** (`fall_water.wgsl`). At distance it widens the sheet to
  2 px with opacity of at least 0.9 so the fall stays a white mark, and
  it pulls the sheet (not the foam or spray) toward the camera by 0.5% of
  its distance past 150 m so coarse ground does not bury it. `--no-falls`
  (`TerrainConfig::falls`) turns them off.
- **The order of the transparent draws.** Rivers and lakes always draw
  before every other blended thing: `RIVER_SORT_BIAS` (-1e5) is the river
  material's `depth_bias`, with the hardware depth bias zeroed in
  `RiverSurface::specialize` so only the sort moves. The spray has
  `SPRAY_SORT_BIAS` so it follows its body.
- **`ProceduralWorldRes`** in `strata-sim` (inserted beside
  `WorldSourceRes` at load) is the handle `stream_falls` reads the world
  from.

### Where it left the design, and why

- The brief said the carve cuts a lip and a plunge pool. It does not: the
  probe at (-35 600, -21 647) shows a straight ramp of slope 1.19 with the
  channel 0.26 m deep in it. The sheet therefore hangs off the face and
  reads as water sliding down a steep face, not a free-falling curtain. A
  real lip and pool is a carve change (a `GENERATOR_VERSION` bump), out
  of scope here.
- Shading the river mesh's fall cells as a fall was rejected (not drawn at
  coarse levels, the zigzag stays); particles with CPU state were
  rejected for the shader-only cycle.
- About 30% of the records have a recorded drop under 6 m, lowered by
  later surface passes, and some exceed `fall_max_m` (505 m at
  (14 281, 42 538), 224 m elsewhere). That is a worldgen question; the
  draw filter leaves those to the river mesh.
- The sheet stays about 1.75 times the channel's width. Narrowing it
  showed the river mesh's own foam cells beside it as white teeth, and
  the mesh cannot be told the draw filter.

### The review's findings, and what became of them

- **Must fix, draw order.** Depth-writing river and fall draws sorted by
  unrelated AABB centres, so a faint fringe of the fall could hide the
  river by camera position. Fixed by the sort biases above; the module
  doc's false claim is replaced.
- **Lip seam.** Fixed: the lip foam runs `LIP_OVER_M` (1.2 m) past the
  lip and the sheet ramps in over `LIP_FADE_S`; the sheet's indices now
  come last in the mesh, since its faint first metre was hiding the foam
  (confirmed with an experiment that turned the body's depth write off).
  A faint soft line remains.
- **Foot's upstream edge.** Fixed with the signed distance and
  `FOOT_UPSTREAM`.
- **Depth pull.** Fixed for foam and spray; kept for the sheet at 0.5%
  because the far ground's error grows with distance. A spur just in
  front of a far sheet can still lose to it.
- **`Crossings` used as a world handle.** Fixed with `ProceduralWorldRes`;
  the far paint's `paint_source` still takes its world from `Crossings`,
  one line to switch.
- **Tests.** Added the foam-keeps-to-its-water test and a seed loop for
  the spray; the shader test checks the shared constants.
- **Look.** Ragged edges with their own noise, stronger streaks, a
  visible spray; the width stays (above).
- **AABB margin.** `far_margin_m` covers the far widening.

### The measures (seed 42, release)

- Build cost about 88 us a fall; on the flight 11 falls within 18 km cost
  3 829 vertices and 965 us in all, at most 6 builds a frame. The rescan
  is not separately timed. The sample's cost is unchanged and no bake is
  triggered.
- 348 falls lie within 80 km of Port Serein, 2 within 30 km.
- Flight A/B under xvfb (`scratch/falls/flight-{falls,nofalls}.log`, 40
  windows): gpu 16.43 ms with and 17.22 ms without; main 8.31 ms against
  7.25 ms, the difference one 64.5 ms window (medians about 6.3 ms in
  both). The frame rate is paced near 16 fps, so this says nothing about
  GPU cost, which is unmeasured. It was not repeated after the fixes.

### Tests

Run again on the finished tree, release, two threads, one binary at a
time (logs `scratch/falls/verify-*.log`): `strata-terrain` lib 92
(`falls::tests` 6), shaders 7, headless 2; `strata-sim` lib 62 (1
ignored); `strata-worldgen` lib 101 (5 ignored), port_serein 5, version
9, snapshots 2, determinism 7 (1 ignored), all unchanged. Clippy
`-D warnings` on `strata-terrain`, `strata-sim` and `game`, rustdoc
`-D warnings` on the first two, rustfmt on every touched file, `cargo
check -p server` and the game's headless run of 10 frames are clean.
Nothing under `crates/strata-worldgen` is in the diff.

### Captures

`scratch/review/falls/`, read after the fixes (all at 2560x1440, `--fog 0`,
time 0.45, poses from `scratch/falls/cap.py`):

- `tall-pool.png`: the 149 m fall from 18 m past its foot. A blue white
  sheet down the whole face in the hill's shade, white churn at the foot
  and down the channel, a soft mist at the foot; reads as a fall but
  glassy, and the sheet is wider than the cut.
- `tall-lip.png`: from 12 m above the lip. The river gathers white and
  goes over the edge, a thin glassy band lies between the foam and the
  sheet; the sea is far below. Reads right.
- `tall-far.png`: 2 km down the valley, 120 m up. A thin pale line down
  the hillside with a second small fall to its right. Reads right.
- `drop20-pool.png`: the 20 m fall from its foot. A white cascade over
  the ramp, foam at the foot and in flecks downstream; the upstream fall
  shows as a white mark. Stepped foam edges at the foot remain.
- `drop20-lip.png`: from above the 20 m lip. White foam into the sheet,
  with a soft lighter line where the hard edge was. Reads right.
- `drop20-far.png`: 2 km down the valley. The creek is a thin blue line;
  the falls are too small to find at this size (see the crop in
  `scratch/falls/crop-drop20-far.png`).
- `r4-tall-mark15.png` against `r4nofalls-tall-mark15.png`: 15 km down
  the valley, 600 m up. At full size the mark is not findable by eye;
  the crops `scratch/falls/crop-r4*.png` show it with falls only.
- `before-tall-{pool,lip,far}.png`: the look before, a plain blue ramp
  with a zigzag edge.
- `showcase.png`: the bridge at (7 404, -4 940) from 120 m up to its
  south-west, looking north-east (yaw -0.785 in cap.py's convention).
  The river winds past the service road, a tributary crosses at the
  bridge, and a thin fall shows on the far wall at upper right. The
  ground is a dark brown in shade here and the bridge is small.

### What is left

- **Look calls for the owner.** The sheet in the hill's shade reads blue
  white and glassy; the far mark at 15 km is a few pale pixels (opacity
  and least width are `FallConfig::far_opacity` 0.9 and `least_pixels`
  2.0); a faint line stays at the 20 m fall's lip.
- **The river mesh still foams its own fall ramp** under the sheet; in
  the 20 m pool view faint teeth show inside the sheet near the foot and
  the foot foam steps where coarse ground cuts it.
- **No lip or plunge** in the ground (above); a worldgen decision.
- **The records over `fall_max_m` and under 6 m** (above).
- **The sort bias** puts every river before every blended draw, clouds
  included, which is right while the river writes depth.
- **GPU cost** of two transparent draws a fall is unmeasured.
- **`paint_source`** can move to `ProceduralWorldRes`.

## Step 8c, as built (2026-10-09)

Wet banks, reeds, roadside markers and lamps (ticket 0004's step 12). Built
in three parts, reviewed (verdict needs-fixes, one must-fix) and the
review's findings fixed, then verified on the finished tree. The ground did
not move: `GENERATOR_VERSION` is still 17, nothing was blessed, and the
pinned `world_version` and the worldgen ground binaries pass unchanged. Not
committed; `just ci`, `just test`, `just server-check` and `just bench` not
run (targeted binaries only, release, two test threads).

### What was built

- **Wet banks** (`strata-terrain/src/mesh.rs`, `bank_of`). A damp band
  beyond the existing wet line: 45% of the line's darkening, fading from
  0.5 to 1.9 m over the surface and out to 4 m past a river's edge reach
  (anywhere round a lake), multiplied by `1 - cleared` so a road's way over
  a ford or a culvert's fill stays dry. It rides in the wet byte the
  vertex record already has, so the shader changed in a comment only. The
  edge reach (`SampleWater::EDGE_REACH_M`, 2.5 m), the wetted bed
  (`WETTED_BED_M`, 0.35 m) and `SampleWater::edge_reaches()` live once in
  `strata-worldgen/src/lib.rs` as new associated items; the terrain and
  the flora both read them. No sample value changes.
- **Reeds.** `assets/flora/reed.flora.ron` (a bush, form `reed`), a
  `veyra-flora` `Habit::Reed` generator (`reed.rs`; tall blades, some dried
  to straw, cattail spikes; levels end at 60 and 240 m), baked by `just
  content` into the library. `GroundPrefs` gained `bank_m` and
  `bank_width_min_m`: reeds stand within 0.7 m over a lake or a river at
  least 6 m wide, never by a fall, thickest at the edge, in clustered
  beds. Everything but rocks and bank plants keeps 0.35 m over the water
  where the edge reaches; in practice only meadow grass moved (0.30 to
  0.35 m). The client drops reeds on a crossing's ground
  (`clear_bank_plants_off_crossings`, fed from `crossings_near` in the
  foliage task): under and past a deck, a ford's way and posts, a culvert's
  apron.
- **Dressing species.** `FloraDef::dressing` marks a species that is the
  client's alone, and `version.rs` leaves it out of `world_version`, so
  adding the reeds did not move the pin. The registry accepts it only on a
  bush with no children and a collider-free form (fern, flower, reed).
  In `strata-flora` a dressing species sorts after every other in the
  candidate list, ranks below every plant in the thinning, never crowds a
  child out, and sets neither the halo nor the thinning bins
  (`FloraTable::exclusion_reach`); `FloraTable::without_dressing()` places
  none, and `FloraPlacement::from_registries` in the physics uses it, so
  neither the server nor the client's colliders see a reed.
- **Markers and lamps.** `veyra_buildings::roadside` (a white marker post,
  a lamp standard; `LANTERN_AT` derived from `LANTERN_SIZE` and
  `LANTERN_CORNER`) is baked to `crossings/veyra/` with its layout at
  `roadside.ron`. `strata_worldgen::roadside::place_roadside` is pure:
  markers every 100 m on both sides of a service road and every 150 m on
  one side by turns on a track; none near a road's ends, crossings, water,
  other ways or pads, or off the road's level; lamps either side 1.2 m past
  each placed bridge deck's end (beyond the pilasters and wing walls, and
  not on another way's carriageway) and one at each site's way in.
  `strata_sim::roadside` keeps `NearRoadside` (4 650 m, each road's pieces
  cached by `RoadId` across refreshes, pruned to the roads listed), and
  `site_groups_version` now hashes the kit. The physics collides pieces
  through `Streamed` (400 m, released at 550 m, by each piece's own
  distance). `apps/game/src/roadside.rs` draws lamps to 4 km and markers to
  1.5 km, nearest first, with a pool of 8 point lights at night within
  300 m (`LANTERN_LUMENS` 10 000).

### Where it left the design, and why

- **Reeds are kept out of crossings on the client, after placement.** The
  brief said to read the crossing record and `place_crossing`; the sample
  cannot tell where a deck is. The server's reed list therefore differs
  from the client's near crossings, which is harmless because nothing
  collides with a reed (and now the server places none). The terrain's wet
  bank keeps off a road's cleared way but not from under a bridge's deck.
- **A dressing flag instead of re-pinning.** Any species added moves the
  pinned `world_version`. The flag keeps the pin and the guarantee in the
  placement (above); a client and a server that differ only in reeds are
  still refused through the tree library's version.
- **Markers draw to 1.5 km, not the crossings' 4 km,** to keep the entity
  count down. Lamps draw to 4 km.
- **"Slow water" is lakes and rivers 6 m wide or more,** because the
  placement record has no gradient; falls are excluded by kind.
- **Reeds stand only above the water line** (nothing grows under water), so
  there are no emergent reeds in the shallows.
- **The roadside fields are not in `CrossingKit`,** which would have
  touched existing literals; the roadside has its own layout file.
- **Lamps stand at bridges whose crossing places a deck** (graded roads
  only, not footbridges) and at roads ending on a site's pad. Port Serein's
  own road and the network's road heads have none.
- **`strata-physics/tests/headless.rs` changed,** outside the brief's
  crates: the rebase test had two `PrimaryObserver` entities once the new
  resource reordered archetypes (a resource is an entity in Bevy 0.19.1),
  and followed whichever the query met last. It now removes the player's
  before spawning the far observer.

### The review's findings, and what became of them

- **Must fix: the dressing flag did not hold.** A reed could take the
  place of a woodland log (a bush with a capsule collider) on the server,
  and nothing in either handshake version covered the reed's fields. Fixed
  by the ranking above, the registry validation, `without_dressing()` in
  the physics, and a test, `dressing_moves_no_other_plant`, that compares
  the full table against `without_dressing()` on 64 cells at the lake shore
  and in the spawn's forest with reeds forced to grow everywhere; it fails
  when the priority term is removed. `a_dressing_species_leaves_the_version_alone`
  covers sparser reeds, no reeds (the version is the same) and the same
  reeds not dressing (it changes).
- **Orphaned comment in `streaming.rs`:** moved back above
  `exclude_occupied`.
- **Refresh cost of the roadside:** the per-road cache and the 4 650 m
  radius (const-asserted against the draw distance, drop margin and
  refresh step in the game).
- **Duplicated constants:** one home in worldgen (above); the game takes
  `LANTERN_AT` from the kit and finds lamps by the kit's `lamp` path.
- **Bridge lamps on another way:** they now check `landform.developed`
  like markers. Declined for pad lamps: they stand on the pad's skirt by
  design, so the check would remove them all.
- **Candidate budget test: declined.** The registry already refuses a set
  over budget (`FreezeError::FloraBudget`); the shipped set uses 3 120 of
  4 096 slots, and dressing takes its slots last.
- **Reed impostor or longer reach: declined.** The last level fades out
  through its visibility range (about 216 to 240 m); a shore past that is
  bare from the air.
- **Look of the reeds: not changed.** Sparse tufts rather than beds, cattail
  spikes barely showing; it needs generator or density changes and is the
  owner's call.
- **Lamp pools too faint:** `LANTERN_LUMENS` raised from 6 000 to 10 000.
- **Per-frame passes over `NearRoadside`** in the physics and the game
  (a linear pass and a `BTreeSet` each frame): a note, left alone.

### The measures (seed 42, release)

- **Placement** (`bank_probe`, release, one thread, sampling included):
  400 cells round the showcase cost 22.57 ms a cell without reeds and
  22.76 ms with (+0.8%, 649 reeds); 100 lake-shore cells 32.26 against
  32.48 ms (+0.7%, 1 928 reeds). The 160 shore cells of the lake at
  (22 208, -4 864) hold 4 539 reeds.
- **Roadside placement:** 1.43 ms for road 54 (service, 15.2 km, 302
  markers). A refresh round the showcase costs 2.5 ms cached (3.8 ms
  uncached), mostly the network lookup; cold 3.7 ms for 8 roads and 277
  pieces. A server and a client placing everything within 2 km agree bit
  for bit.
- **Baked props:** reed near level 712 to 1 434 triangles, far 199 to 322;
  marker 68, 56, 56 and lamp 172, 144, 104 by level. The library holds 17
  species, version `0e674ac947348ce9`.
- **Reed draw cost** (flycam A/B with and without `reed.flora.ron`, 1 500
  frames, median of the last 12 windows, xvfb, `scratch/fix8c/ab-*.log`):
  standing in a bed at the lake shore, GPU 14.85 against 12.50 ms (+2.35
  ms; entities 12 874 against 8 896); 200 m out and 40 m up, 15.25 against
  15.10 ms (+0.15). Main and render CPU unchanged. The levers are
  `REED_UNTIL` and `REED_BUDGET` in `veyra-flora/src/levels.rs`.
- **Roadside flight A/B** (river flight, `roadside.ron` present against
  removed, 24 windows, xvfb paced near 16 fps, so not a benchmark): main
  6.8 against 6.6 ms, render 6.6 against 6.5, gpu 17.55 against 17.5,
  median entities 7 079 against 7 038 (`scratch/dress/flight-*.log`).
- **Wet bank at distance:** stills at 500 m and 800 m show a smooth sand
  band with no stepping. Popping while moving cannot be judged from stills
  and is not verified.

### Tests

Run again on the finished tree for this write-up, release, two threads, one
binary at a time (logs `scratch/dress/verify-*.log`): `strata-registry` 51
+ 1; `strata-flora` lib 11, placement 22 (1 ignored), budgets 1, sky 2;
`veyra-flora` 21; `veyra-buildings` all binaries; `strata-sim` lib 64 (2
ignored); `strata-physics` lib 13, combat 9, headless 16, crossings 1,
sites 1, roadside 1; `strata-foliage` 38; `strata-terrain` lib 93,
shaders 7, headless 2; `content` all binaries including roadside; `game`
34. `strata-worldgen` lib 104 (5 ignored), port_serein 5, version 10,
snapshots 2, determinism 7 (1 ignored), with no pin or snapshot changed.
Clippy `-D warnings` on registry, worldgen, flora, `veyra-flora`,
`veyra-buildings`, sim, physics, foliage, terrain, content and game; rustdoc
`-D warnings` on the nine lib crates; rustfmt `--check` on every touched
file; `cargo check -p server` and the game's headless run of 10 frames are
clean. `Cargo.toml` and `Cargo.lock` carry only the other session's
`veyra-weapons` entries; this change added no dependency.

### Captures

`scratch/review/dress/` (all at 2560x1440, `--fog 0`; shrunk copies in
`scratch/fix8c/small/`):

- `r2-lake-reeds-eye.png`: the lake shore at (22 114, -4 738) at eye level.
  A reed bed at the right, open wet sand ahead; the reeds read as sparse
  tufts and the sand's ripples dominate. `lake-reeds-eye.png` is the same
  pose before clustering (evenly spaced clumps, planted); `r3-lake-reeds-eye.png`
  missed the near bed.
- `r2-lake-reeds-200m.png`: 200 m out over the lake, 40 m up. Beds read as
  patches of tufts along the bars, thinning up the bank; the damp band is
  invisible on the pale sand. `lake-reeds-200m.png` is the unclustered pose.
- `lake-bank-500m.png` and `lake-bank-800m.png`: the shore as a smooth sand
  band, no stepping.
- `r2-road-markers-eye.png`: service road 54 at (12 121, -7 528). White
  posts with orange bands either side about 15 m ahead, the next pair
  small at 115 m. Reads right. `road-markers-eye.png` is the first try.
- `bridge-dusk-eye.png`: the showcase bridge at `--time 0.8` from 44 m
  west; four lamps past the pilasters at both ends, lanterns lit, warm
  pools on the deck and road (readable after the lumen change).
  `bridge-night-eye.png` is the same at time 0.88.
- `r3-bridge-dusk-high.png`: the whole bridge from 28 m up, the four lamps
  and their light on the road; dark overall. `bridge-dusk-high.png` and
  `r2-bridge-dusk-high.png` are misframed earlier tries.
- `showcase.png`: the showcase bridge at (7 404, -4 940) from 120 m up to
  its south-west, looking north-east, `--time 0.45`. The span, its four
  lamps, the winding river with its damp banks and the road either side
  read at a glance; the ground is bare brown with scattered trees, and the
  lamps are small at this height.

### What is left

- **Look calls for the owner.** The reeds read as sparse tufts, not beds,
  and the cattail spikes barely show (levers: `clumps_per_ha` 30 and
  `patch_cover` 0.65 in `reed.flora.ron`, and `reed.rs`). The damp band
  barely shows on the pale lake sand, whose width and ripples are the
  existing bed look.
- **Reed GPU cost** of about 2.4 ms standing in a bed is the worst case
  measured; trim `REED_UNTIL` or `REED_BUDGET` if the shore budget needs it.
- **No reed past 240 m** and no impostor; a far shore is bare from the air.
- **Wet byte unmorphed at level boundaries:** popping or stepping in
  motion is unverified.
- **Handshake:** `site_groups_version` covers `roadside.ron` and its two
  files, and the tree library's version moved with the reed, so clients
  built before this step are refused. `just content` must be rerun by
  anyone pulling the change.
- **Markers' reflectors are painted, not lit.** Lamps light the road from a
  pool of 8 within 300 m at night.
- **Per-frame passes over `NearRoadside`** can be gated on observer
  movement.
- **`bank_probe`** is kept as a probe beside `falls_probe`.
- **Not committed:** the tree holds the other session's weapon work, so
  commit this step by path.
