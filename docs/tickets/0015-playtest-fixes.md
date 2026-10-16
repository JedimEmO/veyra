# 0015: The first playtest's fixes

Status: done 2026-10-16 (steps 1 to 3 built, plan items 1 to 4). The user's words and answers are marked (user);
everything else is a default of mine.

## What the user said (2026-10-15)

> wasm doesn't capture mouse. procurment list not scrollable. weapons not
> aiming in the direction of the cursor (seems maybe reversed?). loading
> performance in wasm abysmall. general performance bad on medium speed PCs.
> travel distances are too large/bike too slow. I don't like the back and forth
> of the quest (need to go to the bench to craft, then travel aaaal the way back
> to the mast??). also, the city and its terrain is too hard to navigate, and
> geometry is blocking the staircases etc. need to clean it up.

Asked and answered:

- **The aim** (user): the shots go where the crosshair is; the gun model in the
  hands points the wrong way.
- **Travel** (user): both. A faster bike (about twice) and the quest's places
  nearer the city.
- **The quest's back and forth** (user): a field workbench at the station, so
  the mast is mended on site.
- **The performance floor** (user): a laptop with an integrated GPU (Iris Xe,
  Radeon 680M class).

## Already fixed

- The training ground is a quest place and the marker leads to it (6bb2793).
- The board's FOR SALE stands above its orders (3f8ca60) and its rows are a
  real scroll panel, wheel and bar (8ffa5c7).

## The plan

1. **Quest flow, travel, aim, scrolling.** A field camp at the station (a
   workbench and a bench terminal) so the mending stage happens on site; the
   station and the channel nearer the city; the bike about twice as fast; the
   gun model pointing where the camera aims; every screen whose list clips
   made a scroll panel.
2. **The city's navigation.** A walkability audit of Port Serein (from the
   quay, can a character reach the Exchange, the board, the terminal, the
   trainers and the training ground, and is every stair clear), then the
   generator fixed until it passes, held by a test.
3. **The browser.** The pointer lock taken on a click and kept; the load time
   measured by phase and cut (the water bake that runs single threaded).
4. **Performance on an integrated GPU.** A graphics preset (Low, Medium, High)
   on the Display tab, Low picked by default on an integrated adapter, and the
   GPU time each preset costs measured on the look fixtures.

## As built

### Step 1: quest flow, travel, aim, scrolling

- Camp: Teague's field camp (`Camp terminal`, `Camp workbench`) stands 16 m back from the mast toward the city, 3.5 m apart
  (`camp_back_m`, `camp_spacing_m` in `places.ron`), found by `strata_sim::quests::camp` on firm ground (nearer in steps, either
  side) and stood from the start of the game, kept after the questline. The mending is Sample, camp workbench, mast, camp terminal.
- Places on seed 42: station 900 m from the city (`station_slack_m` 600), channel 1999 m beyond it (`channel_far_m` 2000) and
  2847 m from the city; the nearest river is about 2.4 km out, so keeping upstream costs about 450 m. Travel per ending 8.2 km
  (was 17.2), not the 6 km aimed at: the city, station, channel, city loop is 5.8 km of river and the sampling detour the rest.
- Bike: road 60, ground 36 m/s (twice), accel, brake, drag and turn raised with them. The scripted ride covers the 1.0 km road
  in 25 s, mean 41 m/s, top 60.0, no crash (the old script circled the road's end at speed; it now advances vertices by
  speed and stops at the end). Per ending on the bike 5.3 to 5.5 min at full ground speed (was 17.4 to 17.7). First hour: first
  made carbine 9.2 to 7.8 min, muzzle 23.8 to 20.1, first harvest 36.1 to 27.5, whole path 41 to 31.
- Aim: the spine bones' X turn leans the body forward, the opposite of the aim's sign, so the gun pitched the wrong way. The
  one negation is now where the rotation is applied (reload, swap, stagger and the hit spring read as authored), the pitch is
  measured from the sim's shoulder (0.45 above the capsule) and the gain is 1.15 (measured, the bones carry about half). Stills
  `scratch/combat/q15b/aim-pitch-*.png` from the behind-shoulder camera; the gun's rest frame is about 5 degrees nose-up.
- Scrolling: journal, harvester, factory, survey, bench rows and the dialogue are `UiNodeDef::Scroll` (wheel, bar); stores,
  sheet, loadout, fitting and crafting keep fixed columns around slotted grids. Wheel checked under Xvfb on the bench.
- Left rough: no still of the harvester or dialogue wheel, the journal's content never overflowed in a still; no unit test for
  the muzzle direction (needs the glTF skeleton); the old ride's mean was not measured; the ending stills in `docs/design/quests`
  name the old distances.

### Step 2: the city's navigation

- Audit: `port_serein_is_walked_from_the_quay` (`apps/game/src/household/tests/nav.rs`, about 16 s) walks a 0.5 m grid over Port Serein
  on seed 42 with the player's capsule (radius, step 0.5, `max_slope` 50 degrees, `PLAYER_HEIGHT_M`) over what the game builds: the
  buildings' level 0 proxies, the lod 0 heightfield, wild and planted trunks, the roadside and crossing pieces, and the three quest
  people as capsules. Household props have no collider and are not read. A cell is standable under a slope limit with the capsule
  clear from a step up to the head; neighbours connect within a step; falls are not counted (stricter). It writes
  `scratch/combat/nav/audit.png` and `audit.txt`.
- Rule: every bench place, the Exchange and the training ground are reached from the arrival; every stair (73, found from the baked
  stair files) is clear foot to head and its head opens onto at least 10 m2 beyond its own box; the main stair runs quay to Exchange.
- Before: the Works foreman stood on the canteen roof; the main stair's last 13 m were buried under a terrace and a pier stood in it;
  lane stair 110 ended in a house wall.
- Generator: the main stair is fitted to the terraces (a flight per bank, flush slabs across each terrace); the transit deck takes its
  height from the stairs it crosses and no pier stands on one; each lane stair keeps a 4 m apron at its head; the canteen moved 2 m out.
- After: 11 of 11 places reached (Exchange 144 m walked for 133 m straight, training ground 480 m for 385 m), 0 of 73 stairs blocked.
  Stills in `scratch/combat/nav/` (before, after, route-*, after-lane-stair-110-head, after-main-stair-terrace).
- Left rough: wayfinding is proposed, not built: the shortest walk climbs the open slope 25 m west of the main stair and joins it at the
  17.7 m terrace, and the arrival faces the sea 35 m from the stair's foot (`quay_along_m` in `places.ron` from -35 to about -10, or
  face the arrival up the stair). The foreman's slot is not reserved by the generator, only caught by the audit. The new head
  aprons reshuffled the house lots (same style, other houses) and leave one 28 m2 back yard cut off. Overlapping name chips on the
  square are a HUD matter.

### Step 3: the browser and the performance floor

- Pointer lock: the page asks in its own `pointerdown` handler while `window.veyraWantsLock` is set; the game reads
  `document.pointerLockElement` back and leaves `CursorOptions` alone on the web. Real Chrome 1228 under Xvfb
  (`scratch/combat/web/drive.mjs lock`): a click gives the lock, mouse moves turn the camera, pause, journal, map and a real Escape
  free it, the next click takes it again, one request a click. Firefox and Safari untested.
- Load (seed 42, localhost, page clock): rivers ready 72.2 s before, 2.2 s now; first frame 76.2 s before, 6.9 s now; other phases were
  not logged before. `xtask bundle` bakes the 128 km natively (9.6 s) into `water.bin` (`hydrology/persist.rs`, stamped by `water_stamp`);
  native keeps its own bake. Download is the cost: bundle 108 MB gzipped (31 before; 133 before the road points were delta coded),
  wasm 31 MB. Throttled Chrome, first frame: 100 Mbit 18.1 s, 50 Mbit 29.6 s (old build about 85 s at 50 Mbit). The 20 s target holds
  on localhost and from about 100 Mbit up.
- Ahead bake on the web: a round is cut into 8 by 8 cells, one a frame, farthest first; past the bundle's reach (about 43 km of riding)
  a cell stalls for the unbaked masses it touches.
- Presets (`assets/look/graphics.ron`, `settings.graphics`, `--graphics low|medium|high`, all live): Low 1024 shadow map, 2 cascades,
  1500 m shadows, no AA, 50 percent foliage; Medium 2048 map, 2 cascades, 75 percent foliage. GPU ms on an RTX 4090 (canopy / dawn / dusk /
  coast): High 3.34 / 3.63 / 2.69 / 2.04, Medium 3.02 / 3.19 / 2.39 / 1.92, Low 2.67 / 2.87 / 2.21 / 1.76; Low saves 14 to 21 percent.
- Default: native IntegratedGpu, Cpu or VirtualGpu is Low, else High; Reset re-picks. Chrome reports `Other`, so the page passes WebGPU
  `vendor/architecture`: Intel and software are Low, AMD RDNA 2 and 3 Medium, else High (here `nvidia/lovelace`: High). Stills in `scratch/combat/gfx/`.
- Left rough: nothing measured on an integrated GPU; no render-scale lever; no bench work time per preset; Medium's saving is near noise;
  Low's 1024 map not measured alone; `water.bin` stays in wasm memory; the download wants a second file fetched after the first frame.
