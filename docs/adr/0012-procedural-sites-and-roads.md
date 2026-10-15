# ADR 0012: Procedural sites and the roads between them

Status: accepted in outline, 2026-10-04 (ticket 0005; the owner asked for "a
preliminary version" of procedural placement of settlements and points of
interest and agreed the shape below). Amends ADR 0009, decision 2.
Decisions 1 to 6 built 2026-10-05 (ticket 0005, step 4; "As built" below);
decision 7 built the same day as the ticket's steps 5 (the network) and 6
(the grade), reviewed and its findings fixed; the crossings' kit is step 7,
built, reviewed and its findings fixed the same day ("As built", step 7,
below).

## Context

ADR 0009 says an author places a city and nothing is searched, because a
search ties a city to the generator: change the noise and the city moves or
finds no site. That holds for a city someone has fitted by hand. It gives a
world with one place in it. `docs/SETTING.md` wants settlements, landmarks
and the roads, service routes and trails between them, and a road needs
somewhere to go. The river network (ADR 0010) already puts one bake below
the render layer, per land mass.

## Decision

1. **Two kinds of place.** An **authored settlement** (ADR 0009) stands
   where its file puts it and never moves. A **procedural site** is found by
   the bake and belongs to the world's version: it may move when the
   generator changes, as a river may.
2. **A site kind is registry data.** `assets/sites/*.site.ron`, a file a
   kind: the ground it wants (slope, height, distance to coast, river, lake,
   pass or crest, biome and province weights), its footprint, the spacing it
   keeps from its own kind and from others, how many a unit of land holds,
   its rank, the class of road that reaches it, and what stands on it.
3. **Placement runs in the bake, after the rivers,** per land mass on the
   same lattice. Authored settlements enter first as fixed points. Kinds are
   placed in rank order: every candidate node is scored against the kind's
   wants, and the best are taken subject to spacing, ties broken by world
   coordinate. The same seed gives the same sites on every machine.
4. **A site has an identity:** an id stable within a world version (kind,
   land mass and node), a generated name, a position, a facing and a
   footprint. The set is hashed into `world_version`.
5. **What a site does to the ground:** a level pad blended into the natural
   ground over its footprint, `developed` so nothing grows on it (ADR 0009,
   decision 4), and a road to its edge. Far smaller than a city's landform:
   a site does not bring terraces or a bay.
6. **What stands on it, for now:** one or two buildings from the existing
   kits and a sign, fitted to the pad by ashlar's own grounding, baked by
   `just content` per kind and placed by the game and the server from the
   site list. A kind later names the generator that fills it; that is not
   this ADR.
7. **Roads are a network between sites,** computed after them in the same
   bake: least-cost routes over the lattice and then refined, the cost made
   of slope, the ground's kind and water, with rivers crossed only where
   they are narrow and lakes not at all. Three classes: a service road
   between major sites, a gravel track to minor ones, a foot trail to
   landmarks. A road has a profile of its own with a greatest gradient, and
   the sampler cuts and fills the ground to it. Where a road crosses water
   the network records a crossing: a bridge by span, a ford, or a culvert.
   Crossings are not sites.

## Consequences

- ADR 0009's "nothing is searched" now reads: nothing authored is searched.
- A mission, a resident or a save that names a procedural site names it by
  id within a world version. A generator change that moves sites is a new
  world for those references; the handshake already refuses a mixed pair.
- The settlement's own road (ADR 0009, decision 3) becomes the first link
  of the network from that settlement's exit.
- The bake grows: sites and routes for a mass are computed with its rivers,
  off the frame.
- Placement and flora read the road and the pad through `Sample`, as they
  read `developed` today, so the server's colliders and the client's
  drawing agree.

## As built (ticket 0005, step 4, 2026-10-05)

Where the build chose otherwise, and why; the ticket's "Step 4, as built"
has the rest.

- **Decision 2.** A kind also names its facing rule (water, view,
  downhill), its naming rules and its building group. A want is the best
  of a few terms and may be required; besides distances to features and
  to other kinds a term can be a measure of the ground (`summit`, `view`,
  `pass`, `cove`). The band must be at least the developed skirt wide.
- **Decision 3.** Authored settlements enter as spacing (each kind's
  `settlements_m`, and Port Serein's extent and a kilometre past it kept
  clear) and their names are reserved; nothing is placed relative to their
  buildings. Each candidate node is refined to the real ground in two
  steps (nine places 170 m apart, then nine 57 m apart round the best),
  in fixed batches, so the sites do not depend on the thread count.
- **Decision 4.** The id is (kind, land mass, node). The set reaches
  `world_version` through the canary networks, whose hashes fold in their
  sites, and the kind files are hashed as well. What stands on the sites
  is the content step's, which `world_version` does not cover: the
  handshake carries a version of the baked groups of its own
  (`strata_sim::sites::site_groups_version`), refused and told like the
  others.
- **Decision 5.** No road yet: each site records its `arrival`, the
  point on the pad's edge a road should reach. Only the yard round the
  pad's centre is laid in gravel, its edge broken up; the rest of the pad
  and a 6 m skirt (fading over 8 m) are cleared, developed ground, so a
  pad does not read as a car park.
- **Decision 6.** A kind's group is 2 to 7 pieces in 4 to 6 arrangements
  (designs and their mirrors), and `place_group`, a pure function of the
  site and the baked layout, stands one on the pad's own height under each
  building's lowest corner rather than by ashlar's grounding, which would
  need the pad's ground in the authoring crates. The board carries no
  text: one baked board serves every site of a kind, so the name is a HUD
  card while the player stands on the site's ground.

## As built (ticket 0005, steps 5 and 6, 2026-10-05): decision 7

Where the build chose otherwise, and why; the ticket's "Steps 5 and 6, as
built" has the rest. The network (step 5) moves no ground; the grade (step
6) does, and bumped `GENERATOR_VERSION` to 15.

- **Which places are joined.** Towns and the far end of Port Serein's road
  by a minimum spanning tree over the travel cost on the surveyed 512 m
  lattice between each and its six nearest within 45 km, plus every edge
  whose detour through the tree costs over twice its own; then each hamlet,
  station and landing, nearest the network first, by a track to wherever
  the network is cheapest to reach (usually a junction on a road); then
  each pass shelter and landmark by a trail the same way. A mass with no
  town grows from its first place.
- **Port Serein's road** is kept whole as the network's first road (its
  points as traced, so the ground it shapes and the city's fingerprint do
  not move), and the grade leaves it to the settlement. Links to the city
  meet it anywhere along it: the backbone forks from it 330 m out of the
  exit.
- **Where a road runs.** A search on the 512 m lattice, then one on 64 m
  nodes in a corridor round that route (384, 256 and 192 m either side by
  class, widened 2.2 times when it finds nothing). The cost is length, the
  gradient against the class's (8, 12, 22 %), the climb, the side slope,
  turning (a reversal dearer still, so a climb winds in switchbacks with
  legs of a few hundred metres and not a sawtooth of single moves), a
  river's meander belt and a texture of value noise that bends a line over
  a plain; the sea, lakes, standing water, other sites' pads and 100 m
  round a settlement's extent are closed; a river is crossed only on an
  edge that crosses its channel once, as a culvert (under 2 m), a ford
  (tracks and trails, centreline depth at `FORD_DEPTH_M` or less) or a
  bridge. Travel on an earlier road costs 0.12 of new ground.
- **The line.** Smoothed in plan within a metre of its searched height, a
  bend under the class's radius tightened within three; a stretch that came
  to cross water, to cross a channel at a fall or too skew, or onto another
  site's pad, a lake or half a settlement's margin, is put back on the
  searched line. An arm ending on another road is eased onto its line over
  the longest of 120, 60 and 30 m that stays clear the same way.
- **The crossings.** Recorded along the final line with banks (at a gorge's
  rim) read on the road's own line, not a straight line off it. A graded
  road bridges a creek in a gorge rather than culverting it; a bridge's
  deck stands half the banks' difference (3 m at most) over the lower one
  and its class's freeboard over the water, its abutments where each bank
  meets it, no further than that bank's rim.
- **A profile per road, at the bake**, service roads and tracks: held to
  the class's greatest gradient, to pads, decks and fords' beds, and at a
  junction level with the through road's crowned surface out past its
  verge; kept over the water beside it but never asked more than the grade
  of a held level; steeper only where the cut or the fill would pass the
  class's most. Trails get none.
- **The sampler cuts and fills** in its last stage beside the carve, from
  the cell the carve reads: carriageway, verge, soil banks, fills, a clean
  gap under a bridge, an arm's mouth widened into the road it meets. A
  river's channel takes none of it but at the road's own crossings. Banks
  of roads and walls of river valleys are soil but where deep and steep,
  in clusters of outcrops: the decision's "steep banks in the province's
  rock" gave way to the owner's wish that cuts read as hillsides.
- **The query** is `Network::nearest_road` and `ProceduralWorld::nearest_road`
  (class, distance, bearing, along, ground), off the sample.

## As built (ticket 0005, step 7, 2026-10-05): the crossings

- **Decided 6 of the ticket** (local stone piers and abutments, a
  prefabricated deck and rails) is `veyra_buildings::bridge`: a road
  bridge's deck, rails and stone abutments are one file per span class
  (every 4 m to 64 m; 32 m for footbridges), piers a file per height, a
  culvert's headwall, a ford's post and stepping stones files of their own.
- **One bake serves every crossing**, as a site kind's group does: the
  shortest class whose gaps hold a crossing's, centred and bedded 2.5 to
  4.5 m into the grade's shelves; piers (thicker as they are taller),
  culvert headwalls (three box heights, the tallest under the road), posts
  and stones placed per crossing by
  `strata_worldgen::crossings::place_crossing`, a pure function of the
  record, the kit's layout and the world's ground, which the server and the
  client call alike.
- **The placement reads the ground and says when it cannot meet it.** A
  graded road's deck is checked against the ground under both its ends
  across the carriageway; where it misses (a curve off the deck's axis, two
  crossings closer than one profile can serve) the next class up is tried,
  then the crossing is `Unplaced::Unmet` and stands nothing. The sim warns
  once per crossing that stands nothing where something was meant to
  (16 of 576 graded bridges within 200 km, the nearest 56 km from Port
  Serein). Meeting them is the grade's, not the kit's.
- **What a version covers.** The kit's layout and files are hashed into the
  handshake's site groups' version, last; nothing was added to the
  protocol.
- **Found on the way, outside the decision:** the cloud shadow's light
  texture (`strata_atmosphere::clouds`) was being blended as a decal over
  standard materials in a metre-thick slab, Bevy 0.19.1 keeping a light's
  texture in the clustered decals' table; its transform now holds every
  point outside the decal box. It painted the crossings' rails red and
  would have painted anything else plain there.
