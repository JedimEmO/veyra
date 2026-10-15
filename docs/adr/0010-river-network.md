# ADR 0010: The river network, the one bake below the render layer

Status: accepted in outline, 2026-10-04 (ticket 0004 steps 6 and 7; the owner
allowed the bake on 2026-09-29 and agreed the order of work on 2026-10-04).
The numbers in it are the design's and are replaced by measured ones as each
step is built. Decisions 1, 2 and 4 to 6 were built on 2026-10-04 (ticket
0005, step 1; the record is "Step 1, as built" there) and decision 3, the
carve, the same day (step 2 and its review, "Step 2, as built"); all are
written below as built.

## Context

The world is a pure function of the point (ADR 0002, ADR 0003), and ADR 0006
turned down a baked grid for rainfall: a million cells before the first
sample, a sampler no longer pure, every consumer carrying the bake. Ticket
0004's research found that rivers cannot be had that way. Carved noise has
no source, no direction and no growth downstream; the erosion filter's lines
stop halfway down a slope; a branching pattern evaluated at a point costs a
hundred times a sample. A river is where the whole catchment above it sends
its water, so it depends on ground arbitrarily far away, and nothing local
to the point can know it.

The land is narrow (no point further than about 18 km from the sea when it
was measured), every land mass is bounded by the sea, and the largest found
is about 186 000 km2. (On the network's own 512 m lattice the largest mass
within 3 000 km of seed 42's origin is 602 169 nodes, about 158 000 km2 and
a thousand kilometres across: a 2 km scan joins masses the finer lattice
keeps apart.)

## Decision

1. **One bake, per land mass, of a network and nothing else.** On a lattice
   of 512 m over the ground before its fine relief, a land mass is found by
   flood fill from any of its nodes (8-connected) and named by its lowest
   node coordinate. The ground there (`ProceduralWorld::coarse_ground`) is
   the continent's base and the range's own metres, every biome relief
   octave at least three lattice spacings long (the first always, so the
   mountains keep their 6 and 3 km octaves) by the biome blend, and a valley
   noise of 6 m plus 12 % of the range's own metres at a 3 km wavelength:
   with one octave a range's flank was a smooth ramp, its water ran down in
   parallel threads and gathered into nothing, and the carve (step 2) cuts
   the valleys this noise assumes. The rain is the climate's moisture (the
   marched rainfall blended with the noise share the biomes see), not the
   bare `rainfall()`, which credits all of a range's rain to its windward
   foot and reads zero on every crest and lee, so the ranges shed no
   rivers. The provinces' shapes, the beds and the erosion filter are left
   out: they shape a slope, not where its water goes, and are most of a
   sample's cost (a node is 866 ns against 2 541 for a sample). Water is
   routed by a priority flood from the coast, ties broken by world
   coordinate so two machines agree. A pit shallower than its province's
   `keep_pits_m` is breached; a deeper one stays a lake at its spill level
   and drains through its lowest outlet. Rain less the province's `sink` is
   summed downstream. Where a province has a grain (`along_grain`, the
   folded belt's 0.7), a step across the fold axis is entered only once
   the flood's level has risen 30 m times the grain times the step's
   share across the axis squared past it, unless the node was reached
   along the grain first: water gathers along the folds and crosses them
   in gaps (built with step 2; without it the carve's valleys, run across
   the grain by the regional slope off the ranges, took the folded belt's
   grain from 2.33 to 1.71). Chains of at least 0.1 m3/s are rivers, cut at
   confluences into reaches with a Strahler order, jittered off the lattice,
   corner-cut three times and swayed sideways (110 m at 1.8 km) with
   polynomial operations only (the crate's float rule). Width is `4.5
   Q^0.5`, depth `0.4 Q^0.375`. Each river's surface is set from the mouth
   upstream so it never rises downstream; a node step dropping at least
   20 m at a gradient over 0.3 is a fall, split into equal falls of at most
   120 m, except a mouth's last step, whose drop is the coarse coast's and
   which the carve grades to the sample's shore. The bake then sets every
   surface again onto the sample's own ground (step 2): the coarse field's
   surfaces stood up to 61 m over it and 107 m under it. Each vertex's level
   is the natural ground less a freeboard of 0.3 m and an incision of 3 m
   plus 8 m per root of discharge (and 12 % of a range's own metres in a
   range), grown in over 500 m from a source; breached from the source down
   (never rising), with a lake's level, the sea and a confluence as floors;
   a breach (something risen in the river's way) graded to a straight line
   to a vertex past it, over the reaches below too, lowering nothing by
   more than half the breach's depth (5 to 30 m); a mouth graded to a
   vertex at the sample's own shore, run on to it up to 600 m. Since
   ticket 0005's step 3b the bake also settles each reach sideways onto
   the sample's lowest ground within 160 m, bends a tributary to join its
   main stem at 40 degrees, holds a lake's outlet at its level to a vertex
   where it leaves the rim (no breach grading or taper lowers that
   stretch), and gives each vertex a meander amplitude and phase: the carve
   winds the channel about the line every twelve widths, with riffles and
   pools, and the line stays the valley's.
2. **The bake is lazy, bounded and hashed.** A mass is baked the first time
   something asks about ground on it, and kept: the coarse field in tiles of
   128 nodes on a pool of at most 16 threads, then the routing on one. A
   mass of more than 2^20 nodes (about 275 000 km2) is cut into its parts
   inside cells of 256 km, each a mass of its own whose cell border is a
   forced divide; whether a mass is over the cap is decided by counting it,
   so its parts are the same whoever asks. No mass met near seed 42's origin
   reaches the cap: the largest bakes whole in about 0.8 s. Hashing every
   mass at a handshake would cost every bake, so what joins `world_version`
   is the bake's numbers, the constants it depends on and the content
   hashes of three canary networks: a small whole island (5 310 nodes, 19
   lakes, falls), two parts of it cut under a small cap (one with a coast,
   one draining over its cut edge) and a window round Port Serein (23 ms on
   one thread). A code change that moves rivers where no canary stands is
   caught by bumping `GENERATOR_VERSION` by hand, which its doc requires for
   any change that moves the network.
3. **The sampler reads the network; it stays a function of the point and
   the seed.** A bucket grid finds the segments near a point (48 to 50 ns on
   a network, 65 to 72 ns through the world with the masses found, against
   about 2 500 for a sample; measured with step 1). The carve is the last stage
   of the sample: it moves heights and leaves biomes, rain and provinces
   where they are. `Sample` carries the water at the point
   (`Option<SampleWater>`, 32 bytes): surface, bed, width, signed distance
   from the centreline, flow (direction times speed), discharge, order and
   kind (channel, lake, fall, mouth); the depth and the speed are derived.
   As built (step 2): a point reads one lattice cell, whose runs of segments
   are scanned once; a point farther from every segment than any of their
   valleys reaches is not carved and pays only that scan. Otherwise the
   segments within reach are blended by eighth-power weights of distance
   (a sum, not the nearest, so nothing steps where the nearest flips), the
   surface a soft minimum of theirs scaled by their spread (so a steep
   tributary does not lift its main stem's bed where they meet). The carve
   cuts a valley (half-width 60 m plus 110 m a root of discharge, by the
   province's section, narrower for a low-order stream by its `hang` and
   across the grain by its `along_grain`, up to 440 m, wider where the cut
   is deeper than the section's steepest wall allows), a flat-bottomed
   channel whose banks rise to the freeboard, filled up to it within a band
   where the ground was lower, and a lake bed levelled to the eased coarse
   ground with a ring at the level. Every number is carried with its
   gradient, so the sample's gradient is the carved height's. A cut over
   150 m fades out by 250 m: the network routes on the coarse field, and
   where the provinces' shapes raise a spur hundreds of metres over it a
   creek would cut a slot up to 1 700 m deep; it runs into the hillside
   instead. Walls show the beds at their own height. Port Serein's sites are
   placed over the uncarved ground (its fingerprint holds) and the network
   routes round them.
4. **The bake lives in `strata-worldgen`,** Bevy-free, because colliders,
   placement, the fog's basins and the server all need the carved ground and
   must agree on it.
5. **Later networks join this bake, not a second one.** Procedural sites and
   roads (ADR 0012) are computed after the rivers on the same lattice for
   the same mass, and are hashed with it.
6. **No river crosses a settlement's own ground** unless its landform
   carries one (ADR 0009): nodes within 450 m of an authored extent carry no
   water and are not routed through, no mouth opens into that margin, and
   the jitter and the sway fade in over the next 700 m. Ground walled in by
   an extent drains to its own lowest node. The nearest river point to Port
   Serein's extent is 2.5 km off.

Rainfall stays marched per sample (ADR 0006 stands).

## Consequences

- The first sample on a land mass waits for its bake. The game and the
  server bake the masses within 128 km of the spawn and of every
  settlement before they build their app and its task pools (84 masses in
  1.15 s on seed 42, the registries and the world built in that), the
  loader's world shares that bake, and the loader bakes round an observer
  on two threads in the background once it comes within a quarter of the
  radius of every square baked so far. A sample that still meets an
  unbaked mass bakes it on its own thread and warns once.
- The world moves once more when the carve lands: heights near rivers,
  everything standing on them, the fog's pinned hashes, the bench fixtures
  and the ledger's baseline.
- A change to anything the bake reads (the continent, the ranges, the rain)
  moves every river. That is the generator's version, already in the
  handshake; a change to the network's own code is a `GENERATOR_VERSION`
  bump too.
- Rivers here are short and steep: gorges and falls off the ranges, not
  lowland giants. Long rivers need land with an interior, a different map.
  Measured with step 1 over 147 800 km2 round Port Serein: the longest is
  48.5 km, the largest carries 8.2 m3/s at 12.9 m wide, 170 km of river per
  1 000 km2, lakes 1.9 % of the land, and 2 431 falls with a median drop of
  98 m, most of them pieces of cascades down a range's flank.
- The sample's budget rises (ticket 0005 allows 1.9 microseconds in the
  mountains, from 1.75). A dry point pays one cell's lookup (the mountain
  strip 1.82 us); a point in a valley pays the scan, the blend and the
  carve, 0.31 us more along the order-2 river south of Port Serein, whose
  own ground already costs 1.96 us.
