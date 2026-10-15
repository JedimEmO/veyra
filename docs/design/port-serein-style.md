# Port Serein: style and geometry

Design, 2026-09-27. How the city in [port-serein.md](port-serein.md) looks
and how its geometry is made. It applies to the rest of Veyra's built world
too; Port Serein is where it is proved.

Decided:

- **Stylised realism.** Real proportions and real materials, simplified into
  bold, readable forms with soft bevels, with painted-feeling materials that
  carry the wear.
- **Moderately heroic scale,** about 1.2 times true.
- **Reusable, highly variable building types.** Every building and every
  part is reusable in principle, across Port Serein and the rest of Veyra,
  but each is a type with many parameters and a seed, not a fixed model.
  Two instances of a type should look like different buildings with a
  family resemblance.
- **Meaningful placement.** Buildings are placed by the site and the
  city's structure (contours, lanes, the square, the quay, views, who owns
  the ground), never on a plain grid.
- **Everything built is a recipe by default.** Buildings, structure and
  props are Rust recipes over ashlar, so they share parts, bevels, materials
  and wear. What recipes do badly (soft, organic, hand-sculpted) is
  modelled in Blender and brought in through a glTF importer, wearing the
  same materials. Local 3D generation (Trellis2, Pixal3D) is for
  characters, creatures and items, not for the city.

## 1. Stylised realism

A player should believe every building could stand and be used, and read
it from across the bay by its shape alone.

- **Shapes before detail.** Every building reads at three distances:
  - Skyline, over 500 m: its silhouette against the sky or the hillside.
  - District, 50 to 300 m: its masses and its big openings.
  - Street, under 30 m: entrances, frames, fittings, wear.

  Design in that order, and do not add street detail to fix a weak skyline.
- **Simplify, then exaggerate a little.** A window frame is a thick, clear
  frame, not a thin realistic one. A concrete pier is a little heavier than
  engineering needs. Thickness is what survives distance and stylised light.
- **Bevel everything a hand could touch.** Soft bevels on every exposed
  edge catch light and read as made. Pelagic concrete has chamfers of 5 to
  15 cm; Landing composites have rounds of 20 to 60 cm; Terrace
  additions are square-cut with small bevels.
- **Detail where people are.** The ground floor and the first two storeys
  carry the fittings: doors, awnings, vents, pipes, signs, lamps. Upper
  storeys repeat plainly, and rooftops carry equipment, which reads on the
  skyline.
- **Materials carry the realism.** Concrete that looks poured,
  composite that looks moulded, paint that has faded. Wear is placed where
  use explains it: salt and runoff below ledges, grime at hand and foot
  height, rust at steel seams. It lives in the material graphs, not in
  geometry.
- **Value structure.** Pelagic buildings are the darkest large masses in
  the city, the Landing modules the lightest, the Terraces in between with
  colour. The processor is dark with bright warning lights. A screenshot in
  greyscale should still read as the city.

## 2. Scale

The character is a 1.7 m capsule, 0.7 m across. It steps over ledges up to
0.5 m and climbs slopes up to 50 degrees, at 5 m/s walking and 9 sprinting.
From a third-person camera true scale feels cramped, so everything a player
moves through is about 1.2 times true.

| Element | Size | Why |
| --- | --- | --- |
| Door | 1.4 x 2.8 m; double 2.6 x 2.8 m | The camera clears the frame behind the character |
| Pelagic entrance | 6 to 12 m high, recessed 3 to 6 m | Monumental: the company controls the approach |
| Storey | Landing 3.4 m, Terrace 3.2 m, Pelagic 4.2 m | Heroic but domestic; Pelagic taller for authority |
| Window sill | 1.0 m above the floor | Real; windows read as windows |
| Stair | Riser 0.18 m, tread 0.40 m, flight at least 2.4 m wide | Easy climbing and visually grand; risers far under the step limit |
| Ramp | At most 1 in 6 (9.5 degrees) | Walkable and pushable by a handcart |
| Lane | 3.5 to 6 m wide | Two people and a handcart pass |
| Terrace | 6 m rise, 12 to 25 m deep | One storey of retaining wall, room for two rows of modules |
| Railing | 1.1 m | Real |
| Transit piers | Deck at 8 to 12 m above the ground | Clear of the terraces' modules |
| Landing pad | 40 m across, cradle 3 m high | A shuttle 30 m long with room around it |

A lane of 5 m with 3.2 m storeys either side stays walkable and still feels
dense. A Pelagic slab of 12 storeys at 4.2 m rises 50 m: tall enough to
dominate the terraces, short of the processor.

## 3. Building types and parts

Everything is reusable; nothing is a fixed model. There are three layers,
each parameterised and each with a seed.

**Parts** (in `veyra-buildings`, by generation). The library every type
draws on:

- **Openings:** door, window and hatch cutters with their frames, in each
  generation's style: a Pelagic slit window with a deep reveal, a Landing
  oval, Terrace shutters.
- **Envelope pieces:** wall panels with seams, parapets and copings, corner
  treatments (Pelagic chamfers, Landing rounds), plinths.
- **Structure:** piers, beams, retaining walls with drainage, stairs,
  landings, ramps, railings, bridges.
- **Services:** pipe runs with brackets, vents, ducts, tanks, cisterns,
  antennae, cable trays, lamps and work lights.
- **Life:** awnings, balconies, lean-tos, planters, stall frames, lamp
  strings, signboards.

Each part has its own parameters (a window's width, height, frame depth and
sill; a stair's rise, run, width and landing count), so one part covers
the whole range the city needs.

**Building types** (in `veyra-buildings` too). A type is a recipe
function: a Landing module, a Terrace house, a Pelagic slab, a warehouse, a
stall, a canteen, a workshop. It takes:

- **A lot:** the footprint polygon it must fit, its slope, which edges face
  a lane, a square, a drop or a neighbour, and the ground's height.
- **A programme:** what it is for (home, shop, clinic, office, store),
  which decides its openings, entrance and fittings.
- **Character parameters:** storeys, massing variant, roof type, how many
  additions and of which generation, age, paint and material variants.
- **A seed,** which picks everything the parameters leave open.

From these it builds its massing to the lot (not a fixed box), puts its
entrance on the edge that faces the way in, its services on the back or the
roof, its windows toward the view, and its additions where the history
says: a lean-to on the lane side, a balcony facing the bay, a tank on the
roof. The same type on a narrow corner lot and on a wide lot facing the
square are different buildings.

**Landmarks** are types too, with one instance each in Port Serein: the
processor, the beacon tower, the terminal, the headquarters. They take
parameters like any other (a second processor elsewhere is the same
recipe with different towers).

**Variety** comes from the parameters, the seed and the materials' own
parameters (paint colour, age, salt). The test for a type is a contact
sheet of a dozen instances on different lots: they must read as a family
and as individual buildings.

## 4. Placement

A city is laid out from its site and its structure, in order, by the
settlement's plan (`veyra-settlements`), deterministically from its seed:

1. **The site's analysis:** contours, slope, aspect, the shoreline, the
   headland, views (to the bay, the processor, the beacon tower), and where
   the ridge and the pass are.
2. **Fixed structure** from the design: the terrace levels on chosen
   contours, the quay line along the shore, the pads on the headland, the
   processor on the ridge, the pipeline cut, the main stair.
3. **Movement:** the service road graded along its switchbacks, the transit
   line level along its contour, lanes along each terrace, stairs between
   terraces where the lanes need joining (every 40 to 80 m, and where
   destinations are).
4. **Lots:** the land left between movement, divided by rules per
   district. Terrace lots are strips between the lane and the wall behind,
   narrow and deep. Lots on the square face it. Pelagic lots are large and
   regular, because the company planned them. The Terraces' lots are
   irregular, because settlers leased them one at a time.
5. **Buildings on lots:** each lot gets a type by its district and its
   position (a canteen on a stair landing, a shop on a corner, a workshop by
   a cargo lift, housing along a quiet lane), and the type fits itself to
   the lot.
6. **Life:** stalls along the square's edges and at stair heads,
   planters and benches where people pause, lamps along lanes, goods and
   carts where trade happens, props placed by the same rules.

The result must explain itself on a walk: why a building faces the way it
does, why a stair is here, why the stalls cluster at the top of the main
stair. Where a placement cannot be explained by a rule, the rule is missing.

## 5. Props

**Props are types too.** A crate stack, a stall, a lamp, a pump housing, a
bench, a handcart, a cargo pod: each is a recipe with parameters and a
seed, built from the same parts library and bevel rules and wearing the
same materials as the buildings. Placement rules put them where use explains
them (section 4). Most of Port Serein's props are hard-surface and
industrial, which is what recipes of solids with bevels do well.

**Three ashlar extensions** cover the soft things recipes of solids do
badly. They are general, so they belong in the library:

- **Sweeps:** a profile along a curve. Cables, hoses, pipe runs and
  handrails; a hanging cable's sag is a catenary.
- **Draped surfaces:** a sheet hung from points with a given sag,
  thickened. Awnings, tarps, banners and canopies.
- **Soft shapes** from signed distance fields, meshed with the surface-nets
  mesher `veyra-rocks` already uses: sacks, bundles, cushions, heaped goods.

**Blender for what needs a hand:** a hero piece, a sculpted statue in the
square, an unusual machine. Blender is not part of the build:

1. Model in Blender and export glTF. The exported file is committed, so a
   bake never runs Blender and is the same everywhere.
2. **ashlar's glTF importer** (new, general) turns it into a prop:
   - material slot names map to our material keys, so the mesh wears our
     graphs rather than its own textures;
   - box-projected metre UVs are generated where the mesh has none;
   - three levels come from decimation, keeping the silhouette;
   - a convex hull collider, or several, is fitted.
3. The prop is written as `.ashprop`, like the trees and rocks, and placed by
   the same rules as recipe props.

The same importer later brings in characters, creatures and items, from
Blender or from local generation (Trellis2, Pixal3D), where consistency
matters less than for architecture and a rig is needed anyway.

**Budgets:** under 1 500 triangles at the nearest level for handheld and
furniture props, under 4 000 for machinery. Every imported prop gets a
review sheet beside a recipe-built neighbour, in the same light, before it
ships.

## 6. The landmarks

Four recipes carry the city's identity and get the most care:

- **The processor.** Two or three exchange towers, 120 to 180 m tall,
  ribbed and buttressed, on an intake hall; the pipeline trunk down the
  hillside; steam from the tower mouths; red warning lights. Visible from
  the whole basin, so its skyline silhouette is designed first.
- **The beacon tower.** The rebuilt survey mast over the Exchange: a slim
  composite shaft with three generations of additions, and a lamp at the
  top. The Landing generation's one tall thing.
- **The starport terminal and control tower.** Pale composite and glass,
  the newest buildings, facing the pads; the control tower's cab overlooks
  the Exchange.
- **Pelagic's headquarters.** The darkest, heaviest building: a block on
  piers over its own terrace, a monumental recessed entrance, enclosed
  bridges to the slabs.

## 7. Budgets

The city is judged by the same frame gates as everything else. Entities are
the CPU's cost (ticket 0001), so the budget counts pieces as well as
triangles.

| | Nearest-level triangles | Pieces at the nearest level |
| --- | --- | --- |
| Terrace house or Landing module | 3 000 to 8 000 | at most 40 |
| Pelagic slab | 15 000 to 30 000 | at most 120 |
| Landmark | up to 60 000 | at most 300 |
| Generated prop | 1 500 or 4 000 | 1 |
| **The slice's city in view** | about 1.5 million | about 8 000 |

Static detail that is never interacted with (trim, services, additions) is
merged per building by ashlar's merge groups, which keeps a building to a
few dozen pieces. Interiors are the lobby-scale ones the slice needs, and
nothing more.

## 8. How it gets made

1. **Blockout.** Terraces, the quay, the pads, the processor and every
   slice building as plain massing, in the world, at scale. Walk it and fly
   it. Fix routes, sight lines and scale before any detail.
2. **Structure.** Retaining walls, stairs, piers, the pipelines, the pads:
   the city's frame, with their materials.
3. **Landmarks.** The processor first, then the beacon tower, the terminal,
   the headquarters.
4. **Types and placement.** The building types with their contact sheets,
   and the plan's lot and placement rules, district by district: the
   Exchange, the Quay, the Terraces.
5. **Props and life.** Stalls, goods, awnings, lamps, signs: prop types
   and their placement, and the few Blender pieces.
6. **Night.** Emissive windows, lamp strings, work lights, pad and warning
   lights.

The blockout is in (2026-09-27). `veyra_settlements::blockout` fits the
slice's share to the city's own ground (ADR 0009): a landform with a bay,
the headland spur and two gullies, terraced in uneven steps. About 600
terrace houses stand in rows along the contours, of uneven widths, heights
and setbacks, broken by gardens, cisterns and lane stairs that zig-zag down
the fall line. Pelagic's slabs are staggered east of the pipelines, which
come down their gully. The quay wall, cranes and warehouses follow the
shoreline. The starport stands a level above the Exchange with the terminal
between them. The main stair runs from the Exchange to the quay, passing
under the transit deck, which follows a contour eight metres below the
square so the square looks over it to the bay. Then the processor and the
pass gate.

The first pass, on a flat grid of the world's own contours, read as a grid:
parallel terraces at even steps, cross lanes lined up across every terrace,
and a hub platform too big to feel like a square. The landform, the uneven
steps, the meandering stairs and the split platform are the answers.

The structure pass is in (2026-09-27), in finished materials while the
buildings stay grey. Retaining walls are traced along every terrace edge,
column by column: each is a solid from the lower terrace's back to the
upper one's front, flush with the upper level and covering the terrain's
own bank, with a parapet on the lane side, built in pieces of at most 12 m
so each piece's convex collider hugs the curve. They open where a stair
comes down and in the pipelines' gully. The settlers' walls are a light
weathered concrete, the company's work a dark cast one. The quay has its
sea wall, coping and bollards; the pads sit in cradles with a hazard rim
and blast walls on the uphill side; the transit deck has piers,
crossheads and parapets; the pipelines run on saddles, each span clear of
the highest ground under it. The city's ground and the road are laid in
gravel, which the terrain now draws: its splat blends towards a developed
sample's own material, which it had ignored, so the road had never shown.

Left for later: walls are traced across columns only, so a bank running
down the slope (between the starport and the Exchange, round a platform's
sides) stays a graded slope; the gravel ends in the extent's rounded
rectangle, which reads from the air; and the gravel is the wild layer's,
darker than a town's paving would be.

Since the fidelity pass (2026-09-27) the city's ground is laid in its own
terrain sets: the terraces' lanes in setts, the quay and the platforms in
flagstones, the banks in gravel (`ground` in the settlement's registry
entry; soil is baked for garden beds). The pavings are read plainly by the
terrain shader (`patterned`), so their courses stay straight.

The landmarks are in (2026-09-27), as recipes in
`veyra_settlements::landmarks` that the plan places. The processor: three
exchange towers of 120, 160 and 180 m, each a flared foot, a waist at
seven tenths and an opening mouth, banded every eighteen metres, eight
buttresses round the foot, a hazard ring at the mouth and red lights at
the mouth and the waist, with gantries between them; a buttressed intake
hall along the platform's front; the desalination tanks; a manifold from
the hall to the pipelines' gully. The beacon tower: the Landing's
composite drum and shaft, the company's steel gallery on struts, a teal
equipment pod and aerials clamped on since, and the lamp. The terminal:
its podium is the wall between the pads and the square, the hall is
glazed both ways with a door each side, the grand stair comes down into
the Exchange, the pad side has a canopy on white columns, and the control
tower's glazed cab looks over the square. Pelagic's headquarters: a block
of slit windows raised on piers over the first deep terrace east of the
gully, a lobby set back under it between two fins, a crown and a mast,
with an enclosed bridge to the nearest slab. The road now arrives beside
the processor, not behind it.

Still to come for the landmarks: steam from the tower mouths (no particle
effect yet); the towers read as industrial stacks more than as exchange
towers, which a crown or intake structure at the mouth would change; and
from the gate, the ground between the road and the city hides the view
the design wants, which a lookout on the road would give.

The first building types are in (2026-09-27), in
`veyra_settlements::types`, starting with the Terraces because they are
most of every view. A terrace house is its owner's choices on a lot:
one or two storeys on the ground, perhaps a module above set back or
shifted with a railing on the roof it leaves, windows and a door cut into
the front (or a roller door for a workshop), a corrugated shed on one side,
an awning over the door, planters or a tank on the roof. Paint is bound
per instance from the design's palette (cream, white, bare concrete,
ochre, green, teal, oxide red), so houses of one shape share a mesh:
about 600 houses come from a few hundred parts. The quay's warehouses
(corrugated sheet on a plinth under a low gable, roller doors onto the
quay) and Pelagic's slabs (slit windows on piers, a stair core, roof
plant) are types too. The Exchange's modules and stalls are next.

A second pass took the blockiness out of the houses (2026-09-27): their
edges are bevelled, or rounded where the owner reused a Landing shell;
windows have sills, hoods, glass and sometimes shutters, doors their leaves
and steps; some have a balcony; roofs are flat behind a parapet, a barrel
vault or a lean-to of sheet, and collect aerials, dishes, panels and stair
hutches. Where a terrace is deep enough, a second row stands along its
front edge with a face to the view and one to the lane, which is what
turns an empty terrace into a street. The retaining walls are battered
and their parapets capped. Variety costs sharing: the houses are now
about 340 000 unique triangles.

The Exchange and the greenery followed (2026-09-27), with a review of the
captures by a second model. The Exchange is the Landing's modules
(rounded composite shells with portholes, storey bands, shopfronts with
painted signboards and awnings, low domes) round a floor of pale poured
concrete, market stalls along the aisles (posts, cloth canopies at varied
heights, counters of boards, crates, baskets, trays and sacks, hanging
signs), shade trees along the modules. Trees are stylised kits (a trunk
and a canopy of soft lobes, a cypress spire, a bush) planted in the
terraces' garden gaps, on their front edges and at the feet of stairs.
Every kit (house, module, stall, tree, crane) is placed through one
mechanism: a part per distinct shape, colours bound per instance.

The review's points acted on: houses now grow in attached clusters of
three to seven with staggered fronts and alleys between (lots are tested
as turned rectangles, not the boxes round them, which had kept diagonal
neighbours apart); deep terraces carry a front row along the parapet and
a middle row, so a terrace is lanes between rows; retaining walls have a
buttress per piece and climbers over some; the cranes are portal cranes
with bracing, a boom, a cab, a trolley and a hook; windows have frames,
shallower reveals and lighter glass; the composite's panels are larger and
calmer; the darkest concretes are lifted. Its point about shadow fill is
left: the sky's environment light is a global look decision.

A second review round followed. Terraces now step 4.5 m on average, not
6, so the walls stop reading as monumental shelves and there are more,
shallower lanes. Climbers are single tapering hulls hanging from the
parapet (the first try, chains of balls, read as dots and cost 400 000
triangles). Stalls have a valance, fuller two-tier counters and lower
canopies. Lamps and benches stand along each terrace's edge, planters and
crates at house fronts. Rust and weathering on the hazard paint and sheet
are toned down, and rooftop vents have bases and caps. Still open from the
review: shaded faces go nearly black (the environment fill is a global
look decision); the city's edge at the ridge meets the terrain in sharp
triangles; the square's paving is large and bare where it should hold
benches and courts.

The cost: the houses are about 685 000 unique triangles in a 92 MB file,
because variety leaves little to share; the GPU time of the bay view went
from about 5.8 to 7.0 ms. The CPU cost of the entity count is not yet
measured.

Props and night are in (2026-09-27). Props: container stacks along the
quay, crates at warehouse doors, washing on a third of the roofs, the
Borough's signposts at stair heads, and on the Exchange a court of benches
and a planter round each shade tree, lamps on the bay side and the water
kiosk. Night: the camera's exposure now follows the sun (EV 14.4 by day,
3 by night, eased through dusk), and a dim cool moon opposite the sun
takes over; the atmosphere crate publishes how dark it is (`Night`).
Bevy draws emission without exposure, so the game scales every building
material's emission by that darkness: lit windows (half the houses, most
of the Exchange's modules), lit shopfronts, the company's lit slits,
lanterns, stall bulbs, rail lamps, pad lights and crane floodlights are
dark by day and glow by night. The layout carries the lamps' and
floodlights' positions, and the game lights the nearest 48 with point
lights, so the quay, the courts and the aisles have pools of warm light.

The city faces north-east onto the bay, so it is lit in the morning and
backlit from noon: captures are taken at `--time 0.34`, and the game's
arrival time should be a morning for the same reason.

Each step ends with captures from the same set of views (the pass gate, the
Exchange, the main stair, the bay) so the city's progress is visible and
comparable.

## 9. Open

- **ashlar work** this design needs: sweeps, draped surfaces, soft SDF
  shapes (sharing `veyra-rocks`' mesher, which moves into ashlar), and the
  glTF importer with material-slot mapping, UV generation, decimated levels
  and hull fitting.
- **Decals** (stencils, painted wayfinding, stains) are not in the engine.
  Until they are, stencils and signs are geometry or material variants.
- **Steam and smoke** for the processor need a particle effect the engine
  does not have yet.

## 10. Colour by place and hour

Since 2026-10-03 the frame is graded by where the player stands and the
hour (`strata_atmosphere::grade`, the lighting plan's phase 4a): each
biome file carries a small `grade`, `assets/look/times.ron` one per dawn,
day, dusk and night, and the two are summed over the look's own contrast
and saturation. Port Serein stands in the coast biome, so the city takes
the coast's cool, clear grade by day and a dusk that the coast tempers.

ACES stays the tonemapper. AgX was tried on 2026-09-27 and paled the
city's paint, and the grade is not a way back to it: a grade can warm,
cool and push or pull colour, but it cannot restore what a tonemapper
desaturates. The rule for the city is that a grade never makes the paint
paler: at the dusk street the red awning keeps its hue within two degrees
and its saturation, and the terrace houses gain colour. Contrast is left
at 1 in every grade, because Bevy's pivots at linear 0.5 and above 1 it
crushed the awning's red from 42 to 33 of 255; a clear place is made with
highlight gain instead.
