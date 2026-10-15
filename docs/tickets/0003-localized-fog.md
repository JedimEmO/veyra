# 0003: Fog that lies in the valleys, and low cloud

Status: step 0 done 2026-09-29, the pass chosen; step 1 (the field) built
2026-09-29; step 2 (the pass draws the field) built 2026-09-29; step 3
next.

*Note, 2026-09-29, updated 2026-09-30: every study-scene and viewpoint
coordinate below is on the ground as it was before ticket 0004's step 2
brought the ranges near. The `valley` study scene has moved since (to a
glen at (-8500, -41000) after step 2's review, and to (34270, 13030) in
step 2b; ticket 0004's fixture tables), so "the study scene" here means the
old plateau at (-1500, -12000). Ticket 0004's step 2b ran a mountain range
through that plateau (1 300 to 2 100 m), and `crates/strata-weather/tests/field.rs`
no longer keeps it: its hollows, knolls and lines moved to the plain 4 to 9
km south-east of Port Serein (the study hollow was (4100, -7700) from that
step's review and is (4000, -8000) since ticket 0004's step 4, which left
the plain's hollows filling 10 to 40 m where one filled 92; the plain
hollow and the knoll between are (11500, -7700) and (7150, -7874) since
step 4's review); the ravine at (-5950, -13650), the open sea and the
city's platform stayed.*

*A known gap, found in ticket 0004's step 2b: on a range's steep flank the
128 m lattice knows the ground less well than on the plain, and fog lies on
ground over its top by more than the 5 m `field.rs` holds on the plain:
29.9 m across the range's flank through the old plateau on step 2b's
ground, 15.8 m since its review, 9.0 m since ticket 0004's step 4 and 12.2
m since that step's review. `field.rs`'s
`fog_on_a_range_flank_is_a_known_gap` holds it under 20 m so it does not
grow unseen; a finer lattice or a slope-aware top on steep ground might
close it.*

## What is wanted

Weather as mood, fog and low cloud first. The pictures to hit:

- Standing in a valley at dawn, the far side gone, trees as grey shapes at
  forty metres, the sun a pale disc.
- From the ridge above it, the valley filled to a level like water, the
  ridges standing out of it as islands, to the horizon.
- `SETTING.md`'s own: an atmospheric processor's warning lights seen through
  low cloud.

Fog is **local**: one valley is full and the next is clear, the coast is
clear at noon and the high basin is not. It moves and it thins through the
morning. It is not a slider on the whole sky.

## What there is

- `strata-atmosphere` draws Bevy's physically based atmosphere, whose
  aerial perspective fades the far ground. `DistanceFog` is wired behind
  `SkySettings::fog_visibility` and off: it is one density everywhere.
- Bevy 0.19.1 has volumetric fog (`bevy_light::VolumetricFog` on the camera,
  `FogVolume` in the world, `VolumetricLight` on the sun). A volume is a unit
  cube, scaled by its transform, with an optional 3D density texture; the
  pass marches a fixed number of steps (64 by default) through each volume a
  pixel sees, reads the sun's shadow cascades at each step and the
  atmosphere's transmittance, and costs again for every directional light.
- The world function already knows where water gathers: `Sample::hydration`
  and `Landform::hollow`. `hollow` looks a couple of hundred metres around a
  point, which finds a dell and not a valley.
- The sim owns time (`TimeOfDay`) and the wind, so every client and the
  server agree on both without sending them.

## Decisions to make, and what I recommend

1. **Where the fog is, is a function, below the render layer.** Fog is part
   of the world: a server decides what a creature can see through it, and
   two players standing together must see the same valley full. So the fog
   field is `(seed, position, time) -> (top, density)` in a crate with no
   Bevy, as placement is (ADR 0005), and the client only draws it. (As
   built in step 1 it takes a `Weather` in place of the time; step 3 is the
   function from the seed and the time to that `Weather`.)
   Recommended: a new `strata-weather` above `strata-worldgen`.
2. **Two drawings, by distance.** Bevy's volumes are right near the camera,
   where the sun's shafts through fog are the picture, and wrong for a
   valley ten kilometres away: a volume per valley to the horizon is many
   volumes a pixel deep, 64 steps each, and past the shadow cascades its
   fog is lit as if nothing shadowed it. Recommended: a pass of our own for
   the landscape (a fog sea has a top and a density that change slowly over
   the ground, which is a 2D field and a few steps a pixel), and Bevy's
   volumes later, near, if the spike shows they add what the pass cannot.
   **The spike in step 0 measures both before either is built on.**
3. **Low cloud is the same medium, higher.** A layer with a base and a top
   in place of a top alone, from the same field and the same pass. Cloud
   that towers, and cloud shadow on the ground, are not in this ticket.

## Design

### The fog field (`strata-weather`, no Bevy)

- **Basins.** On a coarse grid (128 m to start with) the ground's height is
  smoothed over one to two kilometres; where the ground lies under its own
  smoothed height by more than a threshold it is in a basin, and the fog's
  top there is the basin's rim less a margin. This is `hollow` at the scale
  of a valley. It is a property of the ground, computed once per region and
  cached like placement.
- **How much.** Density is the basin's depth, times the ground's
  `hydration` (a wet valley fogs, a dry one hazes), times the weather.
- **The weather** is a slow noise over the map and over days, from the seed
  and the sim's time: some mornings every valley is full, some days only
  the wettest. On top of it the day's own curve: thickest before dawn,
  thinning through the morning, back at dusk. The same function on every
  machine; `strata-worldgen`'s rule about correctly rounded operations
  applies if any of it decides a collision or a creature's sight, which it
  will.
- **Drift.** The field is advected by the sim's wind, slowly, so a bank
  moves along its valley.
- `apps/worldmap` draws the field as one more layer, which is how it is
  reviewed before a frame is rendered.

### The pass (`strata-atmosphere`)

- The field around the observer is kept in two small textures (a clipmap:
  about 8 km at 32 m a texel and 64 km at 250 m), rebuilt on the task pool
  as the observer moves, as the terrain's regions are.
- One full-screen pass after the opaque pass and before the transparent
  one (step 0 moved it after the transparent pass; see below), reading
  depth: a fixed number of steps from the camera to the
  surface (or to the pass's far distance, for the sky), each reading the
  field's top and density under the step, a soft falloff above the top, and
  a slow 3D noise that breaks the top's surface up. It accumulates
  transmittance and in-scattered light: the sun through the atmosphere's
  transmittance with a forward-scattering phase, and the sky's ambient.
- Jittered per frame, since the `baseline` look has TAA.
- No shadow lookups in the first version: inside a fog bank the light is
  the fog's own.
- Budget: one millisecond of GPU time at 2560x1440 on the bench machine. At
  half resolution with a depth-aware upsample if full resolution misses it.

### What it must not break

- The far city's skyline, the tree impostors and the water all take the
  fog: it is a pass over depth, so anything that writes depth does. What
  does not write depth (the water, if it is drawn transparent) is found in
  step 0.
- The server links none of it except the field.
- A render look (ADR 0007) can pin the weather, so the benches and the look
  studies do not change with the time the run started.

## Step 0, measured (2026-09-29)

Both were built behind `--fog-spike` (gone since step 2: Bevy's volumes
were deleted and the pass became the feature) and captured from one real valley, a
ravine at the head of a sea inlet 4.6 km from the `valley` study scene
(floor 28 m, rim 105 m, fog top 85 m), from inside, from the shoulder above
and from 10 km out, at noon, dawn and night
(`scratch/review/fog-spike/`).

**The pass is chosen.** What each drew:

| | Bevy's volume | The pass |
| --- | --- | --- |
| Inside, noon | fog, things gone by 100 to 150 m | the same |
| Inside, dawn | shafts of the terrain's shadow; fog in shadow black | lit, no shafts |
| Night | a black wall | dim, moonlit |
| From above | a white box with straight edges | a fog sea, land as islands, its edge frayed on the slope |
| From 10 km | a crisp rectangle, brighter than the hazed coast | hazed with the coast |
| Water | cannot go under it (the sea writes no depth) | fogged |

GPU time over no fog, 2560x1440, two interleaved runs an arm, under Xvfb
(differences only):

| Arm | Inside | From above |
| --- | --- | --- |
| volume, 64 steps | +0.97 ms | +0.31 |
| volume, 16 steps | +0.28 | +0.11 |
| pass, 24 steps | +1.19 | +0.53 |
| pass, 8 steps | +0.55 | +0.17 |

From above the arms under 0.3 ms are inside the run-to-run gap. The pass at
24 steps is 0.2 ms over its budget from inside and has not been tuned; 8
packed steps showed no banding from inside and faint smudges from above, so
the count between is the first thing to try.

Why Bevy 0.19.1's volumes do not scale to a valley, each from its source:

- `prepare_lights` sorts directional lights with the volumetric ones last,
  and the shader stops at the first that is not: with a moon in the world
  the sun is never volumetric and the fog only absorbs. The spike hid the
  moon by day.
- Every step's sunlight is attenuated by the volume's half-diagonal at the
  local density, `exp(-34)` for a box 2.4 km across. It can be cancelled
  through `light_intensity` at one density only, so a density texture on a
  box that size is blown out or black.
- The ambient term is weighted by the ray's transmittance, so thick fog
  gets none: fog in the terrain's shadow, and all fog at night, is black.
- Steps are spread evenly and long ones are biased dark (a third of the
  right value at 16 steps on a 1.5 km ray), which TAA cannot average away.

What the spike changed in the design above:

- **After the transparent pass, not before it.** The sea is blended and
  writes no depth; drawn after the fog it would lie crisp on top of it.
- **The pass applies the atmosphere's haze itself.** Bevy composites aerial
  perspective before the transparent pass, so anything after it draws over
  the haze. The pass reads the atmosphere's tables, whose lookups are
  copied into `fog.wgsl` and have to follow Bevy's.
- **Steps are packed towards the camera.** On a flat world a ray just over
  the horizon stays in the layer for the whole far distance; even steps
  drew a bright line there. The failure was that line, not banding at the
  top.
- **The far distance is 150 km**, not 40: from 300 m up, 40 leaves a line
  of sea glint on the horizon.
- `fullscreen_material` cannot be used: it binds no depth and no view.

What the field now has to answer, which one altitude could not:

- **The top reads as a plane at noon**, a second sea, whatever the noise;
  at dawn the light along the sun shades the billows and it reads as a
  surface. The field's own variation in the top has to break it, and the
  shading of the top must not depend on a low sun.
- **This world's valleys are shallow.** Within 3 km of the study scene the
  deepest hollow is 25 m; the deep ones are coastal, about 110 m. The basin
  rule has to find a 25 m hollow a kilometre wide, not only a ravine.
- **The sea is not a basin.** With one altitude the whole sea fogs. Sea fog
  is its own weather, a bank that comes ashore, and the field says where.
- **Shafts.** The one thing the volumes add is the terrain's shadow through
  fog at a low sun. A few shadow lookups in the pass, near the camera only,
  is the way to try for it; the volumes' light ordering rules them out
  while the world has a moon.

## Step 1, built (2026-09-29)

`strata-weather`, no Bevy, above worldgen and below the sim, in the
facade and the server's graph. `BasinTile::bake(world, coord)` reads one
8192 m tile on the 128 m lattice with a 44-cell apron (153 x 153 = 23 409
samples, 34 ms on one core, 27 of them sampling) and keeps six channels
per node: ground, the fog's top under full weather, the basin's floor,
hydration, the distance to the sea along low ground and the distance out
over water. `FogField::sample(x, z, &Weather)` gives a `FogSample { base,
top, density }` from baked tiles in about 80 ns. `Weather { fog, sea_fog,
drift }` is all step 3 has to produce.

- **The basin rule** is the grey closing of the ground (the sea's surface
  as ground) by a 2 km disk: the lowest level a disk covering the point
  reaches without its highest ground rising over it. Inside a hollow
  narrower than the disk that is its rim's low point, one level across it.
  The top is `floor + 0.8 (rim - floor)`, the floor being the rim less the
  deepest the ground lies under it within 1 km, so a 12 m trough, a 25 m
  hollow and a 100 m bay head get proportionate tops from one rule. Every
  step is position-local, so tiles meet with the same bits on their shared
  edge (tested).
- **The spike's "ravine" is the head of a sea bay**, not a ravine: the
  ground falls from the 105 m plateau to the water over 500 m in an
  amphitheatre open to the west. The closing finds it (density 0.85 at the
  spike's floor point with no sea fog, top 75 m), but because the bay is
  open its top is not a level: it runs from 63 m at the shore to 97 m at
  the shoulder, a sheet draped on the slope. Enclosed hollows are level
  (the study scene's trough: 161.9 to 162.5 m across a kilometre).
- **The sea is not a basin**: basin fog fades out over the first 400 m of
  water, so the open sea and the straits between headlands are clear with
  `sea_fog` at zero. Sea fog is banks from a 14 km noise read at `drift`,
  a top of 25 to 85 m that sinks to the water at a bank's edge, coming
  ashore over ground under its top along paths of ground under 100 m and
  thinning over 3 km.
- **Density** grows with the depth under the top (quadratic onset from
  2 m, a half at 12 m) times `0.25 + 0.75` of the hydration's wetness, so
  the dry knolls round the study scene haze (0.1 to 0.2) and the wet plain
  hollows fog (0.5 to 0.6).
- `just worldmap` writes a `-fog` layer (brightness density, colour the
  top), with `--fog` and `--sea-fog` for other weathers. The review set is
  in `scratch/review/fog-field/`.

## Step 2, built (2026-09-29)

The spike's pass draws the field. `fog_spike.rs` and its flags are gone;
`fog_pass.rs` and `fog.wgsl` lost the one altitude. Captures are in
`scratch/review/fog/`.

- **Flags.** `--fog F` scales the field's density (default 1; `0`
  registers no pass and no system). `--weather fog,sea_fog` pins the
  weather, default `1,0.5`: every basin at its top, and separate sea fog
  banks with clear water between them, so a coast shows both.
  `--fog-steps N` (default 12) is the one tuning flag kept. The pass logs
  its own GPU time every two seconds (`ground fog: X ms of GPU time`, a
  timestamp span of its own).
- **The clipmap** (`fog_clipmap.rs`). Two 256 x 256 `Rgba16Float` textures,
  8 km at 32 m and 64 km at 250 m, each texel the field's (top, base,
  density) at its centre under the pinned weather and a fourth channel,
  one where its tile was baked. A level's centre is
  the observer snapped to its texel grid, and it is rebuilt around a new
  centre once the observer is a quarter of the half extent away (1 km fine,
  8 km coarse). Tiles bake on the async compute pool, one task a tile, as
  the terrain meshes; a level samples on the pool once its tiles are in
  (65 thousand samples). That pool is Bevy's, a quarter of the cores and at
  most four threads, shared with the terrain, the foliage and the
  colliders: all 81 start-up bakes queued at once put the terrain material
  0.3 to 0.9 s later on this machine, and would have put it seconds later
  on one with a single pool thread. So at most half the pool's threads bake
  at once (at least one), the fine level's tiles are asked for first and a
  level's tiles nearest the observer first. A level still waiting for its
  tiles after a jump starts again round the new place. Re-measured from
  "world ready", three runs each at INSIDE: terrain material 0.46 to 0.83 s
  with the fog and 0.49 to 0.83 s with `--fog 0`, first foliage 2.26 to
  2.44 s against 2.17 to 2.48 s, the same; the fine level is ready at 0.57
  to 0.83 s and the coarse one at 4.0 s (1.4 to 1.6 s before, when it held
  the terrain back). The frame only polls, and the render world copies
  a finished level into its texture (512 KB) in the same extract that hands
  the uniform its corner, so they never disagree; the old level is drawn
  until then. After a 21 km flight west along z = -13 650 (the coarse level
  rebuilt twice, the fine one 14 times, the origin rebased every 2 km) the
  fog on the plain west of the study scene lies where a static capture at
  the same place puts it (`flight-after-recentre.png`, `flight-static.png`;
  the poses match only roughly, the flight's pitch is not the flag's). On
  the flight fixture a fine rebuild took 0.06 to 0.09 s and a coarse one
  0.2 to 0.33 s (measured before the bake budget). The corners reach the shader relative to the render
  origin, computed in `f64` each frame, so a rebase moves nothing.
- **The march.** Each ray is clipped to the band between the lowest base and
  the highest top anywhere in the clipmap (with the noise's reach) and to
  the clipmap's square. A search of 12 reads of the coarse level alone,
  spaced as the square of the index, finds the stretch of the ray that can
  hold fog; the 12 steps are spent there, packed towards its near end
  (curve 3). Without the search, a ray from high ground spent its steps on
  the clear air above the fog and the fog 10 km away was missed. Each step
  reads the fine level (and the coarse one in the 12 % band where they
  hand over) and the top's relief.
- **The top reads as a surface at noon.** The spike's 3D value noise made
  the top a plane from above, because under the top the noise is not tied
  to the surface. The relief is now a height field of three octaves (256,
  64 and 32 m cells) moving the top by 30 % of the layer's depth, 18 m at
  most, so a trough is a trough down its whole column. Two cues shade it
  at any sun height: a second read of the relief 12 m towards the sun gives
  the slope, and the sun's light is scaled by how the slope faces it; and
  the sky's light, and the sun's light scattered many times, are dimmed to
  35 % at the bottom of a trough. From the ridge at noon the bay's fog now
  reads as a lumpy cloud top, lit and shaded (`ridge-noon.png`); at dawn
  the swells are strongly shaded (`ridge-dawn.png`). The noise comes from a
  256 x 256 texture, one filtered read an octave: hashing it cost four
  times the budget.
- **Light at dawn.** Light scattered many times is dimmed by the fog over
  the point, not by the slant to the sun; along the slant a hollow's fog
  was black at dawn.
- **Cost**, the pass's own timestamps, 2560x1440 under Xvfb on the bench
  machine, 12 steps: inside the bay head 0.90 ms (mean of two-second
  readings, which ran 0.48 to 1.25), from the ridge 0.42, from 10 km out
  0.24. Inside is at the budget; the next saving is half resolution.
- **Looks.** `RenderLook::ground_fog` (the F6 panel's "Valley fog"), true
  in every preset, `baseline` included, so the default look, the panel's
  comparison against `baseline`, Reset from a fresh start and a missing
  export's fallback are one look. A look saved before the switch has no
  entry and draws it. The benches run `--render-look baseline --fog 0`
  (`tools/bench.py`), which registers no fog pass and no fog system.
- **The sky fill.** Bevy 0.19.1 copies `AtmosphereEnvironmentMapLight`'s
  intensity into the environment map it generates once, when it first sees
  the camera; the dressing writes 1 and the look its sky fill (4) a sync
  point later, and which one was copied depended on how the executor
  ordered ambiguous systems. The fog pass's systems flipped it: the
  darkened ground and the black band under the fog's edge in the first
  `ridge-dawn.png` were a sky fill of 1, not the fog. `look::sync_sky_fill`
  now copies the look's value down whenever the two differ, so every frame
  gets the look's sky fill (and a sky fill changed in the F6 panel reaches
  the frame). The water and the shaded ground are brighter and bluer
  wherever a run had drawn 1. Recaptured in `scratch/review/fog/fix/`.
- **Not broken.** A multisampled camera gets no pass (one warning), and
  while no camera can draw it no tile is baked. With `--fog 0` (and in the
  benches' configuration) three runs at INSIDE match each other and the
  earlier `--fog 0` captures to the noise floor (25 000 to 81 000 pixels
  differ by more than 8, against 36 000 to 78 000 between runs). They do
  not match the commit before (1.85 million pixels), which drew this view
  with a sky fill of 1 through the race above.

What did not come out:

- **Picture 2 only in part.** From the ridge the bay's slope fog reads as a
  surface, but the bay is open water in the middle; there is no fog sea to
  the horizon with islands, because this world's relief is gentle and its
  valleys few. `--weather 1,1` fills more of the bay with sea fog
  (`ridge-dawn-fullweather.png`). From 10 km (`far-noon.png`) the large
  white patch in the right half is a sea fog bank over open water 4 to 7 km
  out, not the far coast's hollows; the bay head dead ahead shows only as a
  thin whitish line at the waterline, and with `--weather 1,0`
  (`fix/far-noon-basins.png`) the bank is gone and that line is all there
  is.
- **The study plateau's own hollows** are dry by the field (hydration 0.3
  to 0.4, density 0.07 to 0.21), but they do show from 150 m up:
  `plateau-dawn.png` looked west, away from them. Looking east, north-east
  and north from the same place (`fix/plateau-dawn-east.png`,
  `-ne.png`, `-north.png`; `-east-nofog.png` without it) the 12 m trough is
  a sunlit veil over the ground with the knolls' crests standing out, and
  to the north it fills the trough with a long, fairly straight edge
  against the clear knoll to its left. The wet hollows 2 to 4 km west hold
  thicker fog with clear ground between them (`plateau-dawn.png`).
- Picture 1 came out in the wet plain hollow (`hollow-dawn.png`: trees at
  twenty to forty metres as grey shapes, the far side gone) and at the bay
  head (`inside-dawn.png`, a beach with shrubs, not trees). Picture 3 came
  out from above the plain (`two-valleys-morning.png`): the wet hollows
  full, the dry ground beside them clear.
- No shafts (terrain shadow lookups), no drift (step 3), no low cloud.

## Open after steps 1 and 2

- **The cost is the pass's own reading**, 0.74 to 0.90 ms of GPU time from
  inside the fog at 12 steps, from its timestamps under Xvfb, which swing
  about twofold between readings. An interleaved run against `--fog 0` was
  started and stopped unfinished; step 5 owes it.
- **The sky fill.** `look::sync_sky_fill` ended a race in which the look's
  sky fill of 4 often reached Bevy's environment map as 1. Every frame is
  now drawn with 4, fog or none: shaded ground and the sea are lighter than
  in the captures of 2026-09-27 and 28. To go back to the look those
  captures had, the look's fill is what to lower, not the sync.
- **The fog's edge can run straight** where the hydration under it changes
  at a biome's border (`scratch/review/fog/fix/plateau-dawn-north.png`, the
  left edge).
- **Two intermittent tests**, neither the fog's: strata-flora's placement
  binary dies with SIGSEGV in about one full run in three, and
  strata-physics's `tests/headless.rs` fails about one run in five, in a
  different test each time (a walker short of a door's threshold; feet
  half a metre over the ground at tick 0). The second has not been looked
  into, and a server and its clients run that physics.

## Steps

0. **Spike, measured.** In the `valley` study scene: a `FogVolume` sized to
   the valley with `VolumetricFog` and the sun as a `VolumetricLight`, its
   GPU time at 64, 32 and 16 steps and how it looks from inside, from the
   ridge and from ten kilometres; and the barest version of the pass with a
   constant top. Finds the pass's place in Bevy 0.19.1's render graph and
   what does not write depth. Ends with the decision in this ticket.
1. **The field.** `strata-weather`, its tests (the same bits twice, a
   basin where the map shows a valley, none on a ridge or the sea), the
   worldmap layer, and captures of the map for review. Done 2026-09-29.
2. **The pass**, on a fixed weather, with `--weather` to pin it and
   `--fog F` to scale it. Captures in the three pictures above. Done
   2026-09-29, picture 2 only in part (see "What did not come out").
3. **Time and drift:** the day's curve, the slow weather, the wind.
4. **Low cloud** as the second layer, and the processor's lights through
   it at night.
5. **Bench** (`just bench --xvfb`, with and without, interleaved), the
   ledger, ADR 0010, and the render looks updated.

## Risks

- **Banding** where a view ray grazes the fog's top for kilometres. The
  steps are spent by distance through fog, not evenly along the ray.
- **The top reads as a plane.** The 3D noise and the field's own variation
  have to break it; if they do not, the fog sea looks like a second water.
- **Light at night.** Lamps in fog want a glow round them, which is bloom
  or a sprite, and the look has no bloom. Decided in step 4 with captures.
- **The handover** between the fine and the coarse texture of the clipmap.
- **Cost of the basin field** on the flight fixture at 200 m/s.

## Lighting plan phase 2a: shadows, the sun's own light, an air floor (2026-10-03)

Phase 2a of `docs/research/lighting-fidelity.md`: the pass's light, not its
medium. `fog_pass.rs` and `fog.wgsl`; the tunables are on `RenderLook` (not
`GroundFog`, which neither the F6 panel nor an exported look reaches) and on
the panel's Air tab: `valley_fog_shadow` (0 to 1, default 1),
`valley_floor_density` (per km, 0 to 0.1, default 0.004),
`valley_floor_height` (metres over sea level, -500 to 5000, default 400) and
`valley_floor_falloff` (metres, 10 to 5000, default 300), each validated in
`LookFile::validate`. A look saved before them loads with the defaults.

- **The world's shadow.** The pass binds Bevy's view lights uniform (at its
  `ViewLightsUniformOffset`), the directional shadow map array and its
  comparison sampler. The shader declares the head of Bevy's `Lights` itself
  (`ShadowLight` laid out whole so the array's stride is Bevy's; a test
  checks the sizes and the array lengths against `GpuDirectionalLight`,
  `GpuLights`, `MAX_CASCADES_PER_LIGHT` and `MAX_DIRECTIONAL_LIGHTS`), finds
  the sun among the directional lights by its direction (Bevy sorts them by
  its own key and the moon is one), and per step reads the cascade the
  step's view depth falls in, as `fetch_directional_shadow` picks it, with
  Bevy's depth and normal bias along the light and one hardware-compared
  tap. In a cascade's overlap band a per-pixel, per-frame dither picks the
  next cascade in proportion, so TAA blends the seam; over the last
  cascade's band the shadow fades out, and beyond it the fog's own shadow is
  all there is. The sun's single and multiple scattering both take it; the
  sky's light does not. The tap is the pass's dearest read, so it is taken
  on alternate steps (alternating by pixel and frame, the last answer
  standing in), and skipped where the sun gives a step under 3 % of the
  sky's light or what is left to see through is under a tenth. Zero
  strength skips the lookup entirely.
- **The sun's light.** Each step reads the transmittance table at its own
  height for the sun. The light scattered many times (the isotropic half
  that `GroundFog::light` scales) entered through the layer's top, so its
  sun share is now the sun's light on a level surface: times the cosine of
  the sun's zenith angle, a tenth of noon's at dawn. The moon's likewise.
  `light` went from 4.5 to 5.5 to keep the noon fog (5.5 times a noon sun's
  0.82). The ambient is the sky's own radiance: the sky-view table read in
  two rings of four directions, weighted by the cosine to the zenith (the
  light a level top lets in), times `GroundFog::ambient` (1.25 to 0.75)
  times the look's sky fill (4), which lifts the ground's shade by the same
  factor. The multiple-scattering table is still bound and no longer read.
- **The air floor.** A thin exponential height fog, full up to its height
  and thinning by e over its falloff above it, integrated in closed form
  along the whole ray (the march's steps are spent only where the basins
  can be), split at the basin fog's mean distance so the part behind a bank
  is hidden by it. Lit once by the sun and the moon through the fog's phase
  and by half the sky's light, with no sky fill, as the atmosphere's own
  haze is. A ray that meets nothing ends at the sea, or where the planet's
  curve lifts it two falloffs over the floor (about 110 km from 10 m up).
  It lives in this pass, so `--fog 0` (the benches, and every look fixture
  but `dawn`) draws none of it.

**The dawn fixture's white sheet is the sea.** `scratch/look/fog-nofog-dawn-0.27.png`
(`--fog 0`) shows the same bright band as the fog-on capture: it is the
water reflecting the dawn sky, which is the water's matter (the plan's
phase 5), not the fog's. From that pose the fog is a thin band on the far
shore. The fog this phase changes is seen from the plain hollow at
(11500, -7700), which is full at the pinned weather: from 80 m up
(`fog-{before,after}-plain-{sun,south,west}-0.27.png`, `--flycam
--lock-camera --spawn=11500,-7700 --camera-altitude 80
--camera-pitch=-0.2`, yaw -1.2, 3.2 and 1.64) and from inside it at 3 m
facing the sun (`fog-{before,after}-plain-low-sun-0.28.png`, pitch 0.06,
yaw -1.4).

What the captures show (`scratch/look/fog-*.png`, 1600x900, 400 frames,
`baseline` look, `--weather 1,0.5`):

- At dawn the layer is no longer a wall. Before, the plain's fog was one
  flat beige sheet brighter than the sky in every direction (away from the
  sun it measured 130, 107, 76 against a sky of 83 to 112); after, it is a
  soft lavender-blue layer under the sky (44, 48, 64 away from the sun),
  warm and bright on the billows towards the sun, blue in their troughs and
  in the shade.
- At noon it stays bright: the plain from 80 m measures 171, 174, 183
  against 172, 175, 178 before, a shade bluer. The dawn fixture at 0.5 is
  unchanged but for the far shore's haze.
- Shafts: the shadow is read and lands where it should (a debug build that
  drew the shadowed fog red put it behind the crowns and the ridges, as
  rays from the sun), but at dawn it shows mostly towards the sun. Seen
  sideways or away from a 6 degree sun the fog is lit mostly by the sky, as
  it should be once the sun's multiple scattering follows the sun's height,
  so the shadow changes little there; inside the fog facing the sun the
  tree shafts are faint. This world's gentle relief gives few hills that
  stand over a full basin at dawn; a capture with a ridge between the sun
  and a full hollow would be the better proof.
- No seam: no line at the cascades' bounds in any capture.
- The floor: at noon on the coast with no basin fog (`--weather 0,0`,
  `fog-after-coast-floor-only.png` against `fog-nofog-coast.png`) the frame
  moves by 1 to 3 of 255, the far headland a touch softer; the coast does
  not grey. At dusk (0.72, the plain looking west into the sun) it softens
  the far hills and the ground round the sun (`fog-after-plain-west-0.72.png`
  against `fog-after-nofloor-plain-west-0.72.png`, a mean 2 to 3 of 255).
- The canopy at 08:00 is unchanged (no basin there, the floor too thin over
  a few metres).

**Cost**, the pass's own GPU timer (`ground fog: X ms of GPU time`,
`--stats`), 2560x1440 under Xvfb, 1500 frames a run, the median of the
two-second readings after the first 30 s, `baseline` look, `--fog 1
--weather 1,0.5`, runs interleaved. Poses: *inside* is the plain hollow
from 3 m facing the sun (`--flycam --lock-camera --spawn=11500,-7700
--camera-altitude 3 --camera-pitch=0.06 --look=-1.4 --time 0.28
--visual-time 0.28`), *dawn* the dawn fixture's pose (valley study scene,
10 m, yaw 1.5708, 0.27), *canopy* the canopy fixture's (woodland, 1.8 m
facing the sun at 0.333). The stages: *1* is the final shader with the sky
read in two table reads and the sun's light read once a pixel (standing in
for the old ambient's reads) and the floor off; *1+2* the final shader with
the floor off (`valley_floor_density: 0`); *1+2+3* the final shader.

| Stage | inside | dawn | canopy |
| --- | --- | --- | --- |
| before (the committed shader) | 0.716 | 0.232 | 0.160 |
| 1, shadows | 0.911, 0.974 | 0.232 | |
| 1+2, the sun's light | 1.032, 1.055 | 0.243 | |
| 1+2+3, the air floor | 1.034, 1.118 | 0.303 | 0.229 |

Inside, all three add 0.32 to 0.40 ms to 0.716, under the 0.4 ms ceiling
and close to it: the shadow about 0.23, the sun's light about 0.10, the
floor 0.03 (it costs its 0.06 to 0.07 on every pixel, which is what the
dawn and canopy poses show, where little fog is in view). The numbers are
noisy in a way the readings' spread does not show: under Xvfb the RTX 4090
is never loaded enough to hold its clocks and wanders between P2 and P5
(885 to 1890 MHz core, 810 to 10 251 MHz memory, from `nvidia-smi` during
the runs), so a whole run can sit at another clock. A second *before* run
inside read 0.379, with its opaque pass at 1.24 ms against 2.98 in the
others; per reading, as a share of the same frames' opaque pass, the
stages read 0.305, 0.363, 0.389 and 0.411 of it, which is +0.31 ms at an
opaque pass of 2.98. The first build read the shadow on every step and cost
about 0.08 ms more inside: the tap is now taken on alternate steps (by
frame, not by pixel, since neighbours that disagree run it in one warp
anyway) and skipped where the sun's share or the throughput is too small
to show it.

What is open:

- **Shafts want a fixture.** A pose with a ridge between a low sun and a
  full basin, captured from inside the fog, is what would show the shafts
  the plan asked for; none of the four look fixtures has one.
- **The floor goes with `--fog 0`.** If the benches should carry it, it
  needs a pass (or the sky's own shader) of its own.
- **The moon's multiple scattering** now follows its height too, which the
  night captures of step 2 were not taken under.

**After review (2026-10-03).** The air floor's light now reaches the eye
through the air in front of it (the air's mean transmittance over each
stretch of the ray, as for an even air), as the basin fog's does; before,
it was added undimmed, and its extinction is 1.6 times the sea-level
aerosol's at the default haze, so the doc's "under the aerial perspective"
was wrong. Isolated (`--weather 0,0`) on the plain at 0.72 looking into the
sun from 3 m, the floor lifts the horizon band by 4 of 255 at 0.004 and 2
at 0.002, the sky by 1.4; the default stays. Each step's sunlight is also
multiplied by the cloud layer's shadow mask where the ray to the sun
crosses the layer (`clouds::fog_shadow`, the projection Bevy's light
texture uses on the ground), and the floor's by the mask's mean: from 80 m
over the plain at 0.30 with cover 0.8 the fog under a cloud's shadow is a
sky-lit blue and the sunlit gaps white (`scratch/look/p2fix/plain-shadow-*.png`).
The pass now runs in the main pass's set after the transparent pass, so
Bevy's volumetric fog (the local volumes) composites over it rather than in
whatever order the executor picks; the local fog's timing span starts after
it and reads 0.390 ms at the canopy fixture with `--fog 0` and 0.398 with
`--fog 1`, Bevy's pass alone either way.
