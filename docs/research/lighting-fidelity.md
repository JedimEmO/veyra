# Lighting fidelity: the Guild Wars 3 feel, and how to get there

Research for the next render topic: post-processing, atmosphere and
light, with Guild Wars 3's reveal as the reference for feel. Written
2026-10-02 from the public material (there is no gameplay footage and
no technical talk yet), the Bevy 0.19.1 feature set in the registry, and
what `strata-atmosphere` already does. ADR 0007 and
`render-look-study.md` are the earlier look work; ticket 0003 the fog.

## 1. What the reference actually is

Guild Wars 3 (ArenaNet, revealed at Summer Game Fest 2026) is built on
Unreal Engine 5 and set in Orr "a vast wilderness frontier imbued with
the world's magic". The trailer is cinematic: a rider crossing a green
land, floating landforms, cliffs, canopy, a sunset ridge, no HUD. The
official screenshots say what the look is made of:

- A verdant world of floating landmasses, clouds and cliffs under
  midday light: colour and verticality, greens dominating.
- A party on a ridge at sunset: golden hour, monumental shapes emerging
  through haze, warm light against cool shadow.
- A prairie of golden grass under a clear sky; a forest with dappled
  light; a mount with its own luminescence.

Players who know GW2 said it "looked like Veilguard, Avowed and the
other UE5 games". That is the useful observation: the target is the
UE5 open-world look, which has a known recipe:

| Ingredient | What UE5 does | What it gives |
|---|---|---|
| Sky | SkyAtmosphere (Hillaire 2020): physically based scattering, aerial perspective, the sun's colour by elevation | Blue overhead, warm horizon, distance that reads as air |
| Clouds | VolumetricCloud: ray-marched 3D noise, lit by the sun, casting shadows on the ground | Scale, a living sky, light that moves across the land |
| Air | ExponentialHeightFog with VolumetricFog: a froxel grid lit by every light with volumetric shadows | God rays through canopy and ruins, haze in valleys, light that has a body |
| Shade | Lumen: dynamic global illumination and reflections | Shadows that are coloured and open, not black; bounce from a lit wall into a doorway |
| Sun | Virtual shadow maps, soft contact-hardening shadows, the sun disc | Crisp near shadows, soft far ones, a sun you can look at |
| Exposure | Auto exposure with local exposure, a film tonemapper (ACES or AgX), a LUT per zone | Walking from a forest into a field feels like it; each zone has its own key |
| Post | Bloom (convolution on the top end), vignette, a touch of chromatic aberration, TSR upscaling | The glow on the sun, bright magic, the frame's edges held |
| Surfaces | Nanite density, subsurface on foliage, wetness, decals, emissive that blooms | Grass that catches light, leaves that glow against the sun |

GW2's own lighting heritage still matters for feel, and it is cheaper:
its postprocessing stack is bloom, light rays (the sun "will display
rays of light when looked at directly", a screen-space radial effect),
depth blur, ambient occlusion and "selective scene tinting". Its
art directors (Daniel Dociu, Aaron Coberly, Horia Dociu) say the look is
painterly and art-directed over technical: "colour is a language in
itself. It helps set the mood, guide the player's eye", with "soft
greens and golds for serenity, deep purples and cold blues for
foreboding", and that strong value structure and a palette per zone is
what keeps it from dating. The combination to aim for is therefore UE5's
physical light with GW2's colour scripting on top.

## 2. Where the stack stands

From the survey of `strata-atmosphere`, the camera dressing and the
materials (details in the crate docs; measured numbers from
`docs/perf/ledger.md`):

| Ingredient | Ours today | Gap |
|---|---|---|
| Sky | Bevy `Atmosphere::earth` on a planet entity, `ScatteringMedium` scaled by haze, aerial perspective, sun reddening, `AtmosphereEnvironmentMapLight` as the only fill (ambient is NONE) | No sun disc, no clouds. Sky fill is one global number (4.0) and is what keeps shade from going black |
| Clouds | None | The whole ingredient |
| Air | The valley fog clipmap and raymarch (`fog.wgsl`, 12 steps, 0.74 to 0.9 ms): fog where cold air pools, lit by sun and moon, noise on its top. Bevy `VolumetricFog` only in `--study-scene`. `DistanceFog` wired, off | Nothing shadows the fog, so no shafts; no height fog outside the basins; no local volumes under canopy or in rooms |
| Shade | SSAO (medium) with TAA; `ContactShadows` available but off (harsh on grass) | No indirect light at all: shade is sky fill times albedo. This is the single largest difference from the reference |
| Sun | One directional light, 4 cascades, 4096 map, default filtering | No soft shadows (PCSS is behind a feature flag), no sun disc, no glare |
| Exposure | Manual EV100 (14.4 day, 3.0 night, lerped by sun height), ACES, `ColorGrading` contrast and saturation only, AgX tried and rejected (paled the city paint) | No auto or local exposure, no per-zone or per-time grading, no LUT |
| Post | Bloom, DOF, motion blur exist in the look presets but ship at 0; TAA | No vignette, aberration, light rays, sharpening; bloom at 0 means emissives do not glow |
| Surfaces | Forward StandardMaterial everywhere; foliage with `diffuse_transmission` 0.4; water is alpha over depth with no reflection; buildings' emissive scaled by night | No SSR, no specular tint, no wetness, no decals, no rim or subsurface look on grass |

And the constraint that shapes everything: the latest ledger (2026-09-30,
RTX 4090, 2560x1440, `--fog 0`) has GPU at 10.8 to 18.4 ms per fixture
against the 8.3 ms work gate, with the render thread the bound almost
everywhere. Every pass below starts over budget. The study's per-effect
costs (SSAO 0.30, bloom 0.23, fog 0.70, contact 0.13 ms) were measured
on the older, cheaper baseline.

## 3. Moodboard

No images are copied here; each line is a reference to open and what to
take from it. Our own shots are in `docs/design/` and the review
galleries.

**The target**

- Guild Wars 3 official screenshots, in order of use to us:
  https://guildwars3.blog/fr/screenshots/ (descriptions) and
  https://store.steampowered.com/app/4743930/Guild_Wars_3/ (the
  originals). Take: the sunset ridge (haze layering, warm key against
  cool fill, silhouettes stacked by aerial perspective), the floating
  landforms (cloud shadows and scale), the forest (dappled light, bloom
  on the leaf edges), the prairie (grass catching the sun).
- The announcement trailer: https://gw3hq.com/guild-wars-3-trailer.
  Take: the camera's exposure settling when leaving tree cover; the sun
  low and bloomed; the mount's luminescence read against dusk.

**The heritage**

- Guild Wars 2's postprocessing options, the light rays above all:
  https://wiki.guildwars2.com/wiki/Options. Take: light rays as a
  signature, cheap and screen-space; selective scene tinting as the
  precedent for grading per zone.
- GW2 maps to look at for colour scripting: Grothmar Valley (warm
  evening, lanterns), Bitterfrost Frontier (cold blue, bloomed
  braziers), Dragon's Stand at dusk, Verdant Brink's canopy light. The
  art direction interviews:
  https://www.gamedeveloper.com/art/how-the-art-of-i-guild-wars-2-i-is-designed-not-to-look-dated-over-time
  and the Coberly interview on colour as language (Creative Bloq, 2025).

**The techniques**

- Hillaire, "A Scalable and Production Ready Sky and Atmosphere
  Rendering Technique" (EGSR 2020): what Bevy's `Atmosphere` is; the
  shadertoy https://www.shadertoy.com/view/slSXRW shows it alone.
- Wronski, "Volumetric Fog" (SIGGRAPH 2014): the froxel method Bevy's
  `VolumetricFog` and UE5's follow; our valley fog is a raymarch, not a
  froxel grid, which is why shafts need a shadow lookup.
- UE5's sky and cloud setup as a checklist of what one scene needs:
  https://cghero.com/tutorials/skies-clouds-unreal-engine-5.
- Bevy's own: 0.16 atmosphere and decals, 0.17 sun disc, raymarched
  atmosphere, DLSS and light textures, 0.18 atmosphere occlusion (the
  sun's colour now reaches surfaces) and `ScatteringMedium`, 0.19
  contact shadows, physically based SSR, vignette and lens distortion.

**Our own, for before and after**

- `docs/design/city-before-*.png` and the fidelity doc's captures: the
  city at `--time 0.34`, chosen because noon backlights it. That choice
  is a symptom: with no indirect light the lit side is the only side
  that reads.
- `docs/design/progression/hud.png`: the frontier at noon, shade near
  black under the ranger, the horizon's haze the one thing that already
  matches the reference.

## 4. The plan

Six phases, each with a cost ceiling and a capture that proves it, in
the order that moves the feel most per millisecond. Every phase ends
with `just bench --xvfb` recorded to the ledger and the four look
fixtures below re-captured; the F6 render panel (`ui/render.rs`) grows
the new tunables so each can be judged live.

**Phase 0: instruments (half a day).** Four fixtures in
`tools/bench.py` for the look, not the frame time: forest canopy at
08:00, the valley at dawn with fog, the city street facing the low sun
at 16:48 (at 17:30 the sun is already behind the city's hill), the coast
at noon. A `look` ledger section with GPU ms per enabled effect, so
every phase's cost is a diff. A `--look <preset>` CLI flag so a preset
is one word in a recipe.

**Phase 1: the sun (one to two days, ceiling 0.6 ms).**
- `SunDisk` on the directional light, and bloom on by default at the
  study's 0.025 to 0.04 with the threshold set so only the sun, the sky
  near it and emissives bloom. This alone brings the trailer's low sun.
- Screen-space light rays, GW2's signature: a radial blur of the
  sun's occluded brightness from its screen position, as a
  `FullscreenMaterial` (new in 0.18) after bloom, masked by depth so
  terrain and canopy carve the rays. 0.2 to 0.4 ms at half resolution.
- Shadow filtering: `ShadowFilteringMethod::Temporal` with TAA, and a
  trial of `experimental_pbr_pcss` (soft shadows that harden near the
  contact) behind the look file, measured; it is a feature flag and may
  cost more than it gives at 4096.
- Built 2026-10-02 (`docs/research/render-look-study.md`, the dated
  sections): disc, thresholded additive bloom at 0.03 (threshold 2.0 so
  sand and the city's walls at noon do not bloom), light rays as two
  passes of our own (half-resolution march, full-resolution tent) since
  `FullscreenMaterial` binds no depth, temporal filtering by default,
  PCSS present but off (free, and nothing under a size of 2 is softer
  than the temporal filter). About 0.70 ms with the sun on screen, 0.1
  over the ceiling; the Gaussian filter or fewer steps are the levers.

**Phase 2: the air (three to four days, ceiling 1.2 ms incl. the fog
already paid).**
- Shadowed valley fog: sample the sun's cascade shadow map in
  `fog.wgsl` so hills and buildings cut shafts through the morning fog.
  This is the dawn screenshot. Ticket 0003 listed it as the missing
  step; the cost is one shadow lookup per step, so 12 steps is the
  budget and the step curve already packs them near the camera.
- A thin global height fog outside the basins (a density floor in the
  same pass, not `DistanceFog`, which was rejected for reading as
  foreground haze) so every valley has some air at dusk.
- Bevy `VolumetricFog` for local volumes only: under the city's
  arcades, in rooms, under dense canopy, spawned by the settlement and
  the flora placer as `FogVolume`s within 80 m. The near-plane bug that
  stopped the study (issue 25333) has a fix in PR 25336; check whether
  0.19.1 carries it before relying on it.
- Clouds, in two steps: first a 2D cloud layer (noise in the sky pass
  with sun lighting and a projected cloud shadow on the terrain through
  the light's `DirectionalLightTexture`, 0.17's light cookies), which
  gives the moving light and the sky's scale for well under a
  millisecond; a ray-marched volumetric layer is a later topic and only
  if the budget is found.
- Built 2026-10-03 (ticket 0003's phase 2a section; the clouds and local
  fog sections of `docs/research/render-look-study.md`): the valley fog
  reads the sun's cascades (one tap on alternate steps, dithered across
  cascade seams), its sun follows its height and its ambient is the
  sky's own radiance; an air floor in the same pass (0.004 per km up to
  400 m, its light seen through the air in front of it, so it adds a few
  levels of 255 at the horizon, not a veil); a 2D cloud layer at 1.8 km
  whose shadow rides the sun's `DirectionalLightTexture` (dropped under
  0.6 degrees of sun height, where the projection would have relit a set
  sun) and is read by the fog pass too, so a fog bank under a cloud is
  shaded like the ground; Bevy's volumetric fog for the market's eight
  covered runs and for dense crowns within 80 m (6 steps; 0.19.1 lacks PR
  25336, so a camera in a volume's near-plane shell is taken inside it),
  ordered after the valley fog. Cost from the passes' own GPU timers,
  2560x1440 under Xvfb, 1500 frames, median of the readings after 30 s:
  the valley fog 1.09 to 1.18 ms from inside the plain hollow facing a 9
  degree sun (0.28), the same with the clouds on and off; 0.35 at the
  dawn fixture (1 local volume in view, 0.14) and 0.27 at the canopy
  fixture, where the local volumes are 0.39 (3 in view); the cloud layer
  about 0.03. The card held a low clock in these runs (the opaque pass
  2.8 to 3.7 ms), so the absolute numbers run high; inside the fog the
  valley fog is 0.39 to 0.41 of the opaque pass either way. Open: a pose
  that shows the shafts, the local volumes and the light rays ignoring the
  cloud layer, the clouds' grey hearts.
- Clouds reshaped 2026-10-03 (the cloud shapes section of
  `docs/research/render-look-study.md`): heaps of three sizes gathered in
  groups by a 24 km coverage field, soft frayed edges, bodies shaded by a
  read towards the sun, and the sun's disc, halo and rays go behind a
  cloud (`clouds::SunVeil`); 0.04 to 0.06 ms at a working clock, 1.5 times
  the first layer. The canopy fixture's camera now stands in a cloud's
  shadow at 08:00, so its ground has lost the sun (a third of Phase 2's
  light; as bright as before with the clouds off).
- Clouds given volume and motion 2026-10-03 (the clouds as volumes
  section of `docs/research/render-look-study.md`): still one 2D image
  and no ray march. The volume is shading (the density read as a height
  field `cloud_thickness` tall, lit faces wrapped, bases lit through the
  body by its optical depth, a silver lining on the thin sunward parts,
  sky and ground bounce) and a second deck at 2.6 km on its own tile,
  the nearer deck sorted forward over the farther one; the motion is a
  22 m/s wind, the upper deck at 0.8 of it veered 20 degrees, and edges
  that fray as they drift (`cloud_evolution`). Four new `SkySettings`
  fields and a faster `cloud_speed`, validated and on the F6 panel.
  Both decks 0.13 to 0.14 ms at the coast and the zenith, against 0.05
  to 0.08 for the reshaped layer (Xvfb, 2560x1440, the card at one
  clock).

**Phase 3: the shade (four to six days, ceiling 1.0 ms).** The largest
gap and the one Bevy does not hand us. Three options, in the order to
try:
- Sky visibility baked into the world: the terrain mesher and the flora
  placer already run per 64 m cell off the frame; add a sky-occlusion
  term (a handful of rays against the heightfield and the tree library's
  crowns) stored per vertex and per placed tree, read as a bent normal
  and an ambient scale in the terrain and foliage shaders. It costs
  nothing per frame and turns the flat sky fill into fill that is
  darker under crowns and in gullies and open on ridges.
- An irradiance volume per streamed region: a low-resolution 3D texture
  of spherical harmonics generated from the sky and one sun bounce off
  the ground's albedo, uploaded as Bevy's `IrradianceVolume` light
  probe. Gives coloured bounce (warm ground under a warm sun) for the
  cost of a texture lookup. The generator is ours, on the task pool.
- Solari (ray-traced GI) is RTX-only, experimental, and its crate is not
  in our registry; it is the right long-term answer for the city's
  interiors and is worth one spike on a branch, not a plan line.
- Built 2026-10-03 (the sky visibility and irradiance probe sections of
  `docs/research/render-look-study.md`; Solari not tried): the sky's
  visibility baked per terrain vertex (eight azimuths out to 64 m at LOD
  0, with a bent normal) and per placed plant from its neighbours'
  crowns, read as diffuse occlusion, which made the mesher 8 to 10 %
  slower at LOD 0 to 4 and costs nothing on the frame; and irradiance
  probes per LOD 0 region near the player (8 x 4 x 8 ambient cubes of the
  analytic sky and one sun bounce off the ground, horizons to 1 km, 9 to
  11 ms a region on the pool), the two nearest a point ahead drawn and a
  200 km sky fallback for the rest. After review the two split the sky by
  elevation, so a gully is darkened once (the probe keeps only what its
  near horizon leaves; in the darkest ravine near the spawn a shaded wall
  keeps 0.109 of linear blue against 0.090); a drawn probe fades to the
  fallback's light on sides with no drawn neighbour and fades in and out
  over 0.6 s; and the clusters end at the farthest light or drawn probe,
  not at the fallback's 346 km (186 m at the street at night, where the
  opaque pass is 2.5 % shorter against SSAO's time). Cost: the opaque
  pass 0.06 to 0.26 ms more with two drawn (0.11 to 0.17 normalised by
  SSAO, in-run A/B), the clustering pass 0.01 to 0.02; at 2560x1440 under
  Xvfb, 1500 frames, the opaque pass 3.48 ms at the canopy fixture and
  1.96 at the dusk street, clustering 0.17 (the card at a low clock
  again). Open: the look ledger's GPU means rose (canopy 9.59 to 11.04,
  dawn 8.00 to 9.14) in runs that also carry the new clouds, so not the
  probes' alone; buildings and trees are not in the probes' heightfield;
  only two regions get the bounce; the coast fixture's sand breaks its
  ripple pattern along a tile edge (in Phase 2's capture too).

**Phase 4: exposure and colour (two days, ceiling 0.2 ms, raised from
0.1 on 2026-10-03 for the auto exposure; see the note below).**
- `AutoExposure` (Bevy's, histogram-based) against the manual EV curve,
  judged on the forest-to-field walk; keep whichever reads right, and
  clamp it so the city at 17:30 cannot blow out.
- Colour scripting: a `ColorGrading` section per biome and per time of
  day, blended by the same climate sample the terrain uses (the
  `Sample` already carries biome weights), so the coast is cool and
  clear, the woodland green-gold, dusk warm, night blue. This is the GW2
  lesson and it is cheap; it is also where AgX gets a second hearing,
  since a per-zone grade can give the city paint back what AgX took.
- Vignette (0.19) at a low setting; chromatic aberration only on the
  hurt and the fall, which the HUD already knows.
- Built 2026-10-03 (the colour scripting and exposure sections of
  `docs/research/render-look-study.md`): a grade per biome (an optional
  `grade` in each `assets/biomes/*.biome.ron`, checked by the registry)
  and per time of day (`assets/look/times.ron`), blended under the
  observer and eased over a second onto the camera's `ColorGrading`, a
  few thousandths of temperature and tint because Bevy's white balance
  is strong; ACES stays, the dusk street's paint keeps its hue and
  saturation, and AgX was not reopened: a grade can warm, cool and push
  colour but cannot restore what a tonemapper desaturates
  (`docs/design/port-serein-style.md`, section 10). Bevy's histogram auto
  exposure is the default, as a correction on the manual curve that only
  brightens (band -1.5 to 0 EV, key -4, the open field at 08:00 in a
  cloud's shadow): the forest floor at 08:00 metered nearly two stops
  under the ground the manual curve was tuned on and read black, and
  auto lifts it 0.7 to 1.4 stops while the four fixtures, the street
  away from the sun and the city at midnight stay within 0.04 stops of
  manual; a symmetric band darkened the coast at noon by 1.55 stops and
  took half of a cloud's shadow away. After review the band closes by
  night (`auto_exposure_night`, 0): with it open a night wood was lifted
  1.5 to 1.7 stops towards a cloudy morning; closed, the night is the
  manual curve's and the histogram pass is dropped. A vignette at 0.18
  under the HUD (the panels' glass 2.5 to 3 levels of 255 darker, the
  text untouched) and a chromatic aberration at 4 taps driven by the
  hurt and held at half while down, nothing at rest. Cost at 2560x1440
  under Xvfb from the passes' timers: by day about 0.16 ms (auto
  exposure 0.113, effect stack 0.04, the grade 0.01 in tonemapping),
  0.17 with the aberration on, 0.05 at night. The plan's 0.1 was set
  before Bevy's histogram was known to read every pixel with no
  resolution knob; the ceiling is raised to 0.2 rather than auto made
  opt-in, because auto is the part of the phase that fixes a visible
  fault. `exposure_mode: "manual"` gives the 0.11 ms back if Phase 6
  needs it. The look fixtures (`Phase 4 colour` in the look ledger):
  the dusk street's market paint has the saturation of Phase 3's capture
  (0.257 against 0.262 over the stalls), the coast cooler and its
  corners 14 levels darker, nothing popped; the GPU means fell 3.8 to
  4.2 ms from Phase 3's, the card's clock and not the phase. Open: the
  grade cannot tint the shade (lift and gain are
  grey), so a warm dusk takes some of the shade's blue; the coast
  fixture's sand still breaks its ripple pattern along a tile edge.

**Phase 5: surfaces (three days, ceiling 0.8 ms).**
- Water: `ScreenSpaceReflections` needs the deferred prepass, which our
  forward materials refuse; so the trial is a planar or a cheap
  sky-only reflection in `water.wgsl` plus specular tint, with foam at
  the depth edge. The reference's lakes and the coast fixture depend on
  it.
- Foliage: a rim and transmission pass on grass (the bent normal from
  phase 3 feeds it), so grass catches the low sun as in the prairie
  shot; emissive flowers and the city's strips at a bloom-visible level.
- Wet ground after rain through clustered decals, once weather over
  time (ticket 0003 step 3) exists.
- Built 2026-10-03 (the water and the grass and emissives sections of
  `docs/research/render-look-study.md`): the water is our own, since
  Bevy's environment specular is scaled by the sky fill of 4 and made the
  lakes a white sheet. One 256 x 256 seeded wave image (forty cosines,
  made at start-up through `libm` so every machine gets the same bits)
  read twice, as a swell and a chop that drift by the sim's clock along
  its wind and fade into roughness from 90 to 300 m; the sky's reflection
  read from the environment map at intensity one and kept two degrees
  over the horizon, Schlick with a specular tint that leans blue, the
  sun's glitter through the cloud shadow and the sun's cascades, the air
  in front of the water added by the shader since Bevy's aerial
  perspective covers only opaque depth, and foam on the crests and at
  the shore. The world's coasts drop to the sea bed within a metre, so
  the shore's band also searches the depth prepass for land up to 8 m
  along the world's axes, inside 400 m of the camera (an `if` after
  review; a `select` had run it to the horizon, worth about 0.01 ms at
  sea level, where water past 400 m is an 18 px strip). Grass gains a
  backlight term (transmission and an edge-on rim against a sun behind
  the blade, through the blade's own face normal, the cascades and the
  phase 3 sky term; `grass_rim` on F6), about 0.01 ms; in shade it adds
  nothing. Windows and lamps now cross bloom's threshold of 2 by night
  (a brightest channel of 3 to 4) and stay far under it by day
  (`DAY_GLOW` 0.02). Cost from the passes' timers at 2560x1440 under
  Xvfb: the transparent pass, water and clouds, 0.20 to 0.23 ms with the
  sea filling the frame (0.10 over the old plane), 0.14 at the coast
  fixture, so the phase costs about 0.11 ms over Phase 4 (the water's
  0.10 at worst, the grass 0.01) against its 0.8. The look
  fixtures (`Phase 5 surfaces`): the dawn lake a dim mirror of the
  sky, not a sheet; the coast a deep blue sea with whitecaps inside the
  waves' fade and a pale band at the waterline; no grass glowing in the
  canopy's cloud shadow; no window or lantern lit in the dusk street. The
  GPU means rose 1.9 to 3.0 ms on Phase 4's, at every fixture alike and
  with the coast's transparent pass at the same share of the opaque pass
  as before, so the card's clock and the machine's load, not the phase.
  Open, and the halo needs the user: at bloom 0.03 a 12 px window lifts
  its ring by 0.05 levels of 255, below run-to-run noise (three times the
  emission gives 0.41, bloom at 0.1 gives 0.23), so the choice is
  emission near 10 with a day share per material, a stronger bloom, or a
  lower threshold by night. Also open: the shore's band stops where its
  probe 8 m landward leaves the screen (the coast fixture's near
  waterline, lower left), the far dawn lake reads as a darker slate strip
  under the warm horizon, emissive flowers were not done, the windows'
  warmth is lost to the tonemapper at night, and wet ground waits for
  weather over time.

**Phase 6: paying for it.** The gate is 8.3 ms and the GPU is already
at 11 to 18. Before phase 2 lands, one profiling pass on the current
frame (the render thread is the bound, so first the CPU side: batching,
the foliage entity count, the impostor batches) and a decision on
upscaling: DLSS is in 0.19 behind a feature and needs the SDK, RTX and
Vulkan; there is no FSR in Bevy, so the fallback is rendering at 0.8
scale with TAA and CAS sharpening. Target: the look fixtures at 1440p
inside the gate with phases 1 to 4 on.
- Profiled 2026-10-03 at Phase 4 (`docs/perf/profile-2026-10-03.md`):
  the render thread is the bound and almost none of its cost is drawing.
  It is per-entity work over tens of thousands of foliage entities that
  are not on screen (three resident mesh levels per tree and an impostor
  batch per species per far cell), the per-batch-set GPU preprocessing
  for a few hundred sets, the sun's cascades and mesh uploads that never
  settle. Upscaling touches none of that. Recommendation: no upscaling
  now; in order, impostors as instance records and the warm-up, one mesh
  level per plant with merged far batches and no wireframe plugin, one
  grass material, the shadow distance, together 5 to 8 ms of
  thread-summed CPU. Once the GPU is the bound, a 0.8 render scale with
  CAS behind a `RenderLook::render_scale` field first, DLSS a spike
  after it.
- Built 2026-10-04 (the profile's closing section, `What phase 6 did`,
  and the cascades section of `docs/research/render-look-study.md`):
  the profile's 4.1 to 4.4 and two of its lines, no upscaling. Far trees
  are records in one shared buffer drawn through thirteen shared quad
  meshes, so a far cell creates no `Mesh`; a plant holds the mesh level
  it draws (two through a crossfade) instead of all three; grass is one
  material over shared blade and cell buffers, one batch set a pass
  (ADR 0005's amendment of this date); the wireframe plugin is added only
  with `--wireframe`; the sun has three cascades to 4 km, each drawing
  only the casters whose swept shadow reaches its slice; the sky's
  environment map is 256 a face and filtered only when the sun turns a
  degree, the observer climbs 50 m or the sky settings change; `--stats`
  and the bench summary carry the sun's shadow span, the batch sets, the
  foliage counts and the mesh uploads; and a static bench fixture warms
  up until the foliage streamer is idle (48 to 49 s) and then measures
  3000 frames. After review the buildings cast into every cascade again
  by default (`building_shadow_cascades` 4; a tall building's shadow
  under a low sun lands in the near cascades whatever its distance, and
  with the trim the cut saved nothing at the street), and the reach that
  a lower count trials is taken from the camera's own field of view. At
  2560x1440 under Xvfb (`tools/bench.py`, settled), the render thread's
  p95 is 6.8 ms at the woodland and 7.1 at the city street (the profile's
  19.9 and 26.6 at `--fog 0`, the Phase 5 ledger's 15.6 and 16.9 with
  streaming in it), its p50 5.9 and 6.4 (11.8 and 11.6); the main
  thread's p95 5.8 and 8.9 (9.3 and 16.8); entities 20 902 and 27 790
  (56 670 and 40 962); batch sets 211 and 238 (487 and 419). On the
  moving fixtures, which keep the ten-second warm-up, the flight's render
  p95 is 13.0 (15.0 in Phase 5) and the walk's 13.5 (15.4). The look
  fixtures (`Phase 6 render thread`) match the cascades section's
  captures over the lower half of the frame within 0.3 to 0.8 levels of
  255 (mean absolute difference); against `Clouds v3` the dusk street is
  9.8 levels brighter and warmer and the canopy 3.5, as is every capture
  taken since, because those were made before the clouds' commit changed
  the cloud shadow lying over the sun at that hour. The GPU means doubled (16.8 to 18.9 ms),
  every fixed per-pixel pass with them (SSAO 0.22 to 0.92 ms): the card
  drops its memory clock on the lighter frame under Xvfb's pacing, as the
  cascades section measured. Open: the settled render thread is under the
  8.3 ms gate at both static fixtures, the flight's (13.0, its terrain
  and colliders streaming) is not, and the main thread at the city street
  is 8.9; the woodland's prepass rose more than the other passes (4.4 ms
  against 0.65) and wants a GPU A/B on a display at fixed clocks before
  it is read as the clock; the profile's 4.5 (masked overdraw) and the
  render scale wait for the GPU to be the bound.

## 5. Decisions to make before starting

1. Realism or paint: the reference is physically lit with painted
   colour on top. Phase 4's per-zone grading is where that is decided;
   the first three phases are the same either way.
2. The hardware floor: Solari and DLSS want RTX. If the floor is a
   2060-class card (GW3's own minimum), Solari is a spike and the
   baked sky visibility is the shipped answer.
3. Clouds: a 2D layer now, or wait and do volumetric once. The 2D layer
   is a day and gives the cloud shadows that sell scale; it is also
   throwaway.
4. Whether to fork `VolumetricFog` for the local volumes or keep every
   volumetric in our own fog pass. ADR 0007 said no fork; the fix
   upstream may have made the question moot.

## Sources

- ArenaNet reveal coverage: https://www.invenglobal.com/articles/22526/arenanet-returns-guild-wars-3-unveiled,
  https://fandomwire.com/guild-wars-3-is-finally-here-with-a-trailer-at-summer-game-fest-2026/,
  https://www.guildwars3.com/en, https://store.steampowered.com/app/4743930/Guild_Wars_3/
- Screenshots described: https://guildwars3.blog/fr/screenshots/; trailer: https://gw3hq.com/guild-wars-3-trailer
- Player reaction on the look: https://steamcommunity.com/app/1284210/discussions/0/570414394495804153/
- GW2 art direction: https://www.gamedeveloper.com/art/how-the-art-of-i-guild-wars-2-i-is-designed-not-to-look-dated-over-time,
  https://tech.yahoo.com/articles/why-guild-wars-2-remains-130000038.html;
  options: https://wiki.guildwars2.com/wiki/Options
- Hillaire 2020: https://onlinelibrary.wiley.com/doi/abs/10.1111/cgf.14050; https://www.shadertoy.com/view/slSXRW
- UE5 sky, cloud and fog setup: https://cghero.com/tutorials/skies-clouds-unreal-engine-5,
  https://dev.epicgames.com/documentation/en-us/unreal-engine/volumetric-cloud-component-in-unreal-engine
- Bevy releases: https://bevy.org/news/bevy-0-16/, https://bevy.org/news/bevy-0-17/,
  https://bevy.org/news/bevy-0-18/, https://bevy.org/news/bevy-0-19/;
  volumetric fog: https://github.com/bevyengine/bevy/pull/13057, the near-plane fix https://github.com/bevyengine/bevy/pull/25336
