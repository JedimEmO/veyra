# ADR 0009: Settlements are placed and shaped by the world function

Status: accepted, 2026-09-27. Amended the same day, three times: a terraced
hillside in an oriented frame replaced the flat disc; then an authored
placement and the settlement's own landform replaced the site search, so the
world's generator can change under a city (decisions 1, 2, 3 and 5); then
the city's ground took its own materials and its plants became the
wilderness's (decisions 6 and 7). Amended 2026-09-28: the layout names a
skyline (below).

## Context

The first world slice needs a city: a small metropolis about 200 m across,
with streets a player walks and lobbies a player enters. A building is fitted
to the ground by ashlar's `fit_ground`, with foundations and ramps (the
grounding study). A city is too large for that. Two hundred metres of hillside
under a street grid would be all foundations, and the ground around it would
never meet it cleanly. The world is also a pure function a server must agree
on bit for bit (ADR 0002, ADR 0003), so whatever receives the city has to be
part of that function, not an edit laid over it.

## Decision

1. **A settlement is registry data.** `assets/settlements/*.settlement.ron`
   says where the city stands (`at`) and which way it faces
   (`downhill_deg`), the extent of ground it owns, its landform, its
   terraces (the quay's level, the mean step between levels and how much it
   varies, how much of each step the wall takes), its level platforms,
   where its road leaves and how far it runs, and the content step's layout
   file. The next settlement is a file.
2. **The author places a city; nothing is searched.** A site search over
   the world's noise ties the city to the generator: change the noise and
   the city moves, or finds no site. So the city stands where its file puts
   it, in a **frame** with its origin there, `+z` down the slope towards
   the water and `+x` along the shore. Everything inside the city is in that
   frame.
3. **The city brings its own ground.** Inside the extent, `sample()` uses
   the settlement's **landform**: a hillside profile down the slope, from
   ridge to seabed; a bay, the ground rising towards the extent's sides so
   the waterfront curves; spurs and gullies running down the slope; and
   relief seeded by the settlement, stretched down the slope so it reads as
   spurs and gullies rather than blobs. Across the falloff it blends into
   whatever the world made there. The landform is then quantised into
   levels: the quay up to its height, then a level every step, each step's
   rise jittered by the settlement's seed, joined by steep smoothstep walls.
   Because the levels are the landform's height rounded, every terrace runs
   along its own contour without anyone drawing it. Platforms (the
   starport, the Exchange, the processor) are rounded rectangles at their
   own level, or the landform's mean over their footprint, blended into the
   terraces. The road is traced from its exit over the blended ground and
   does not reshape ground inside the city. Every sample away from a site
   pays one bounds test. The retaining walls, stairs and quay edge are
   buildings; the terrain only has to be level on each terrace and steep
   between.
4. **Nothing grows on developed ground.** The sample's landform carries
   `developed`, and placement refuses any plant or rock where it is over a
   half. Placement is shared by the server's colliders and the client's
   drawing (ADR 0005), so the two agree.
5. **The city is content, fitted to its own ground.** The content step
   places the site from the registry and has `veyra-settlements` fit the
   city to `Site::city_ground`, where `Site::owns` says the ground is the
   settlement's whole: the quay wall along the shoreline it finds, the
   transit deck along a contour, stairs down the fall line wherever a wall
   takes a flight, rows of houses along each terrace. No world is built for
   it. It bakes each district and writes `generated/settlements/<id>.ron`:
   which baked file stands where in the frame, `y` a world height, and the
   `Site::fingerprint` of the ground it was fitted to (a hash of the frame,
   extent, landform, terraces and platforms). The world loader leaves the
   buildings out, with a warning, when the fingerprint differs; the server
   builds colliders from the baked files with plain `ashlar`, and the
   client spawns the same files.

6. **The city lays its own ground.** A settlement names three terrain
   layers (`ground: Some((lanes, aprons, banks))`), and the sample's
   `material` on developed ground is one of them by `ground_kind`: a
   platform and the band round it are apron (flagstones), ground steeper
   than a twenty-fifth is bank (gravel), and the rest is lane (setts). The
   terrain draws every layer's share interpolated per vertex (ADR 0003's
   splat, since amended), so where two laid materials meet the border
   follows the ground and not the triangles. The layers are not in the
   fingerprint: a new paving moves no building, so it needs no new fit.
7. **The city's plants are the wilderness's.** The plan does not model
   trees. It writes `plants` into the layout, each a flora registry species
   at a point, a scale, a turn and a variant. `strata-flora` places them
   in their 64 m cell with everything else (`planted_in`), so the physics
   gives them the library's trunk colliders and `strata-foliage` draws
   them as it draws the forest, near and far, in the wind. Decision 4 still
   holds: nothing grows on developed ground on its own; the plan plants
   what grows there.

## Consequences

- The world is the seed, the registries, the tree library and the
  settlement layouts. The layout is not yet in the replication handshake: a
  client with another layout collides differently from its server. It joins
  the handshake when the content step versions all of its output together.
- The world's generator can change freely. Inside the extent the city's
  ground does not move, so neither does a building; only the blend at the
  city's edge and the road outside follow the new noise. A change to the
  settlement's own file changes its fingerprint, and the city is left out
  until `just content` fits it again.
- A city put somewhere unsuitable, far inland, still gets its bay: the
  landform dips below the sea there and the sea plane fills it. Where a
  city goes is the author's call.
- Buildings stand on the terraces with no foundations: the terrain is level
  under every lot the plan accepts, which it checks at the lot's corners.
- The layout's plants are part of the world as much as its buildings: a
  client and a server with different layouts disagree on trunks.
- `find_land_near` and the default spawn are unchanged. `--at city` and
  `--at road` start a player at the settlement, in the game and on the server.

## Amendment: the skyline, 2026-09-28

Ticket 0002. A settlement's buildings stream in within 2.5 km of it and the
terrain draws to 48 km, so from further off the bay was bare ground.

- **The layout names a skyline.** `SettlementLayout::skyline` is an
  optional path to one more baked file,
  `settlements/<id>.skyline.ashlar`, beside the buildings' directory so no
  building's name can be its: the whole city as one building, every
  planned building simplified hard (nothing under 4 m, a quarter of the
  segments, no interior, chamfered boxes plain; `far` elements such as
  lamps kept by ashlar's rule, and two kinds kept as the buildings'
  farthest level keeps them, at 1.5 m: every element bound to a
  definition that glows, so the city is as lit either side of the switch,
  and elements at least 16 m long and 1.5 m wide, as lines) and placed in
  the settlement's frame,
  baked as one
  concatenated level (`LodCell::Whole`), so it is one entity per material
  binding and side. The field defaults, so a layout baked before it still
  loads. `veyra_settlements::skyline` builds the recipe from the plan; the
  content step writes it through the same cached bake as every building.
- **Where it stands.** Its file is in the settlement's frame: the game
  places it as a building at the frame's origin (`Site::origin`, world
  height zero, so its heights are world heights) turned by the frame's yaw
  (`Site::yaw`), which is `to_world([0, 0, 0])` and `world_yaw(0)` of the
  placed settlement. A content test proves it: every turned building's share
  of the skyline fills, in the world, the box that building's own file fills
  where the game places it.
- **It is not one of `buildings`.** The physics and the server build the
  city's colliders from that list; the skyline draws and collides with
  nothing, and carries no collision proxy and no portal (the content step
  reads the written file back to check). Listing it there would put a
  second, invisible city of colliders on top of the first.
- **It is not in the replication handshake**, like the layout it is named
  by: a client with another skyline draws another distant city and
  collides the same.
- **The game draws a settlement far or near, never both.** A range is
  measured to each entity's own origin, so the skyline and the buildings
  cannot hand over by range without drawing some of the city twice and
  some not at all. Far, the skyline draws; near, the buildings, their last
  level to any distance. The game (`apps/game/src/settlement.rs`) measures
  the observer to the rectangle bounding the layout's footprints: buildings
  stream in, hidden, inside `STREAM_M` (2.2 km), and the whole city goes
  near inside `NEAR_M` (1.7 km) once every building has spawned, back to
  far past `NEAR_M` plus `HYSTERESIS_M` (1.9 km), and the buildings stream
  out past `DESPAWN_M` (2.7 km). While far, every piece of the buildings
  is `Disabled`, so hidden pieces cost no frame's visibility or
  extraction; they are enabled on the frame the city goes near, before
  that frame's propagation. The skyline is present inside the
  terrain's view distance plus 2 km. A skyline that is missing, loading or
  failed stands in for nothing, so while it is not spawned the buildings
  that are in draw whatever the state, and the skyline is hidden: it draws
  exactly when the buildings do not, so its pieces arriving on a frame the
  buildings still draw cannot double the city.
- **Each building's farthest level is joined too.** The game's ladder
  (`content::ladder`) is ashlar's default with its last level, from 250 m
  on, concatenated into 80 m cells: the city's far level went from 7 272
  placed pieces to 1 040 batches, for the same 0.45 M triangles drawn.

The first bake (2026-09-28) was 35 pieces and 249 153 triangles, 22.1 MB,
six times the ticket's target of 40 000: the Terraces' houses were 165 000
of them, because their walls were chamfered boxes and their parapet caps and
roof plates are broad enough to pass any feature size under their own width.
With plain boxes it is 174 849 triangles and 16.7 MB (2026-09-29), and
with its lit elements kept 180 973 and 17.1 MB (the same day). The
content test holds the skyline to a ceiling of 181 000 so it cannot grow;
the target stays 40 000, and bringing it there is a kit change or one to
ashlar's rule for thin broad slabs (ticket 0002). The joined far levels add
another 28 MB to the city's files, which were 88 MB and are 132 MB with the
skyline.

## Amendment: finite neighbourhood benches, 2026-10-01

The original contour quantisation cut matching steps through the entire
hillside, including its outer blend ring. From the hillside it read as a
striped extrusion. The landform now derives uneven, finite neighbourhood
leases from the settlement seed. Each lease shifts its levels slightly and
can omit an intermediate level, merging two narrow benches into a deeper
one. Lease ends follow a low-frequency bend and feather back to the uncut
landform. Rounded bank profiles vary by lease and level; undeveloped gullies
and the side blend ring keep the underlying slope. The quay, authored
platforms and road pass retain their established levels.

All sampling still uses correctly rounded arithmetic and seeded value
noise. Gradients measure the final height function, including the lease
ends. The ground fingerprint now includes an algorithm revision as well as
the registry inputs, so a shaping-code change invalidates an old bake.

The content plan traces actual flats, keeps short retaining walls only at
steep banks, reserves cross-lanes and small courts before housing, and tests
turned lots at their corners, edge middles and centre. Wide benches admit
sideways infill. There is no change to the runtime layout format, authority
or crate layering. Shared street fittings live in `veyra-buildings`; their
placement and planted bank dressing remain in `veyra-settlements`.
