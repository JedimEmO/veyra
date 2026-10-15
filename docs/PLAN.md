# sci-fi-scrolls implementation plan

Status: v0.11, 2026-09-12, Phases 0 to 4 done, integrated and reviewed, Phase 5 (vegetation) planned from the foliage discovery and spike (`docs/research/foliage-moodboard.md`, `docs/research/foliage-poc.md`, ADR 0005): the slice as described in section 1 runs at 200 fps and more at 1440p on the dev machine, with a character walking on the physics ground, the first slotted screens over it, and a dedicated server two clients walk on together, each seeing the other and its own connection on the HUD. Targets Bevy 0.19.1 and the local `bevy-slotted` workspace at the same Bevy version. Companion documents: `docs/adr/*.md`, and `README.md` for how to run it.

## 1. Goal and non-goals

**Goal.** A game engine on Bevy for a sci-fi MMO-like RPG: Elder Scrolls exploration and freedom, Star Wars Galaxies' player-driven world, KotOR's companions and dialogue, Anarchy Online's implants and mission generator. The engine is a workspace of layered crates; the game is a thin app on top.

**Setting direction (2026-09-13).** [Veyra: a world still being made](SETTING.md)
defines the game's inhabited frontier, its ongoing terraforming, and the player's
arrival in the Serein Basin. Anarchy Online's world and atmosphere are the main
setting influence. Veyra's names remain working names.

**Vertical slice (this plan).** One binary that opens a window on a seeded, procedural, seamless world several tens of kilometres across, with distinct biomes, a free camera, and a draw distance to the horizon at a stable frame rate. Everything in it must already sit on the boundaries the MMO needs later: world generation runs identically on a headless server, the simulation ticks on a fixed step, and the client is a renderer over that.

**Non-goals for the slice.** Combat, inventory beyond an empty hotbar, dialogue. (Networking transport and a walking character were non-goals until Phase 4 added both; the rest stands.) Caves and overhangs (the world is a heightfield with room for cell-based interiors later, see ADR 0003). Vegetation and prop scatter beyond a hook. Water simulation. Save games.

### 1.1 Setting-driven work after the landscape slice

The next destination is an inhabited Serein Basin, following the setting's
[first playable proof](SETTING.md#engine-direction-and-first-playable-proof).
This is future gameplay work; the phase completion records below describe the
existing engine slice.

Build toward a journey from a small Port Serein district to a service settlement
and a monitoring station, in this order:

1. Authored settlements and landmarks within procedural terrain, with stable
   identities, approaches, and vegetation clearings.
2. Roads, trails, and service routes connecting destinations, then the entrances,
   buildings, and interiors those places need. These give the props and interiors
   deferred to Phase 6 a concrete setting and purpose.
3. Residents, service providers, affiliation, dialogue, and usable machinery.
4. Missions tied to those people and places, with saved, server-owned local
   consequences over the seeded world.

The journey's acceptance target is meeting a resident, accepting a job,
operating station equipment, returning for a response, and revisiting the saved
result. Shared equipment state must agree for two connected players, survive
location reloads and server restarts, and reach players who arrive later.
The setting document describes the route's visual and narrative criteria.

Buildings use a Rust recipe pipeline. The authoring system moved out of this
workspace into [ashlar](../../../bevy-crates/ashlar) (ADR 0008, superseded there
by ashlar ADR 0001): the Bevy-free `ashlar` domain crate provides the recipe
types and builders, the separate `ashlar-manifold` adapter evaluates solids,
and `building-preview` runs ashlar's viewer over the game's own scenes with
orbit and wireframe controls. [The game's side](BUILDING_RECIPES.md) is the
content and the grounding fixture. The [material study](research/building-materials/README.md)
adds explicit chamfers, independent PBR libraries and shared facade/entrance parts
used in two layouts. Integration into world streaming remains future work.

The Rust outpost now has [terrain adaptation](research/building-grounding/README.md):
level placement, segmented foundations, generated entrance access, static collision
and vegetation clearance at three repeatable seed-42 sites. The pure fitter lives
in `ashlar`, shared study content in `veyra-buildings`, and shared Bevy
mesh/PBR upload in `ashlar-bevy`. Static collision proxies are derived from the
elements that declare them, so no hand-typed collider table can drift from the
geometry. Character traversal and origin rebasing
are tested, and all six in-world captures passed using synchronous pipeline
compilation; the study recovery notes document earlier driver compiler crashes. Building streaming/LOD and moving doors remain future work. The
[implementation plan](BUILDING_TERRAIN_PLAN.md) records the delivered scope.

Keep Veyra's specific content in the game layer over reusable engine systems.
Large environmental transformations remain a later design topic; the first
journey needs local interactions and persistence. Detailed subsystem designs
belong in follow-up RFCs and ADRs as this work reaches implementation.

## 2. Principles

Carried over from bevy-slotted where they apply, with the ones an MMO adds.

1. **The world is a pure function of the seed.** `sample(seed, x, z) -> Sample` lives in a crate with no Bevy and no IO. The server, the client, the tests and a map tool all call the same function and get the same bits. Determinism is a test, not a hope.
2. **Domain never knows about rendering.** World coordinates, biome logic, the simulation tick and the streaming decision ("which regions are wanted") are plain Rust. Meshes, materials and cameras are adapters over them.
3. **Server and client are plugin groups over one simulation.** From day one there is `headless()` and `client()`, the way `SlottedPlugins::server()` exists. The slice runs both in one process; the split is a launch flag, not a refactor.
4. **Ports for every boundary with two implementations.** Authority (local now, networked later), world source (procedural now, procedural plus persisted edits later), asset source (plain dir now, layered packs later via `slotted-packs`).
5. **Registries freeze.** Biomes, terrain materials and later items, species and quests are namespaced data loaded in a data stage, then frozen. `slotted-registry` already does this; we use it rather than write another.
6. **f64 in the world, f32 on the GPU.** Positions in the simulation are `f64`. Rendering happens relative to a movable origin. No entity ever carries a large `f32` (ADR 0002).
7. **Every heavy job is asynchronous and cancellable.** Mesh generation, sampling and later asset loading run on the compute pool with a budget per frame. The main thread only dispatches and collects.
8. **Testable without a GPU.** Streaming selection, LOD choice, origin shifts and world sampling all have headless tests. A `just ci` that runs in CI without a display is the gate.

## 3. Workspace layout

```
sci-fi-scrolls/
├── Cargo.toml                    workspace root, no [package]
├── justfile                      check, test, lint, run, server-check
├── rust-toolchain.toml
├── .cargo/config.toml            local [patch] of bevy-slotted to the sibling checkout; ashlar is a path dependency next door
├── crates/
│   ├── veyra-buildings/          no Bevy: the game's architecture, its grounding contract and its study textures, over ashlar
│   ├── strata-core/             no Bevy: ids, seeds, WorldPos (f64), RegionCoord, ChunkCoord, deterministic RNG, units
│   ├── strata-registry/         no Bevy: the game's registry set on slotted-registry: biomes, terrain materials, later everything else
│   ├── strata-worldgen/         no Bevy: noise stack, climate fields, biome classification, height sampler, WorldSource port
│   ├── strata-weather/          no Bevy: where fog lies, basins baked per 8 km tile and sampled under a weather (ticket 0003)
│   ├── strata-sim/              Bevy, no render (bevy_ecs, bevy_app, bevy_state, bevy_time, bevy_transform, bevy_tasks): fixed-tick simulation, the Authority port, world-space transforms, the world loader
│   ├── strata-stream/           Bevy, no render: quadtree over the world, interest management, wanted/loaded region sets, budgets
│   ├── strata-physics/          Bevy, no render: avian3d stepped in the sim tick, a heightfield collider per lod 0 region, the character controller
│   ├── strata-terrain/          Bevy render: heightfield mesh builder, async meshing, LOD skirts, terrain material, origin-relative transforms
│   ├── strata-atmosphere/       Bevy render: sky, sun, fog, day cycle bound to sim time
│   ├── strata-client/           Bevy render: cameras, input, the player capsules (local and remote), the slotted UI and HUD, the client plugin
│   ├── strata-net/              the transport and replication port: NetRole, NetStatus, Replicated, Transport
│   ├── strata-net-replicon/     Bevy, no render: the bevy_replicon adapter, a renet UDP server and client, interest by region
│   └── strata/                  facade: StrataPlugins::{client, headless}, prelude, feature flags (`net-replicon` adds the adapter)
├── apps/
│   ├── game/                     the slice binary: window, the character, `--headless` runs the sim only, `--connect` joins a server
│   ├── server/                   the dedicated server: the headless facade served over UDP, no window, no renderer
│   └── worldmap/                 CLI: renders the biome, height, moisture, hydration, cover and fog maps of a seed to PNG; the worldgen dev loop
├── assets/                       biomes/*.biome.ron, terrain materials, textures, shaders, the UI theme, fonts and icons
├── tools/
│   └── xtask/                    dev commands that do not fit just
└── docs/
    ├── PLAN.md  adr/  research/
```

Every crate is a workspace member and inherits edition, lints and shared dependencies. The Bevy dependency is the workspace's, `default-features = false`, and each crate turns on only the features it needs, so a `cargo tree` of the headless group never contains `bevy_render`.

### 3.1 Dependency graph

```
strata-core ◄── strata-registry ◄── strata-worldgen
     ▲                 ▲                   ▲
     │                 │                   │
     └──────── strata-sim ◄──── strata-stream ◄──── strata-physics ◄──── strata-net-replicon
                    ▲                  ▲                   ▲                        ▲
                    │                  │                   │                        │
                    ├── strata-atmosphere                 │            strata-net (port)
                    ├── strata-terrain ◄──────────────────┼──── strata-client
                    └──────────────────────────────────────┘

                    no renderer: core, registry, worldgen, flora (Phase 5, no Bevy at all), weather (no Bevy), sim, stream, physics, net, net-replicon
                    bevy_render: terrain, atmosphere, foliage (Phase 5), client (client's `ui` adds bevy_ui and slotted)

                    strata (facade: plugin groups, feature flags)
                    apps/game, apps/server, apps/worldmap (the only places that see everything)
```

Rule: an arrow never points from a lower crate to a higher one. `strata-stream` decides what should exist and does not know what a `Mesh` is. `strata-terrain` turns wanted regions into meshes and does not know why they were wanted. `strata-worldgen` does not know what a region is; it samples points. `strata-sim` depends on the registry and worldgen crates because it hosts the world loader (`WorldLoadPlugin`) that turns the frozen registries and the seed into the `WorldSource` every crate above reads; that arrow points downward, so the rule holds.

`strata-weather` (ticket 0003) is Bevy-free and sits above worldgen and below the sim, like flora: where fog lies is part of the world, since a server decides what a creature can see through it, so it is a pure function of the world, the seed and a weather state that the server links and a renderer only draws.

`strata-flora` (Phase 5a) is Bevy-free like worldgen and sits beside it, above the registry and worldgen: it places the vegetation of one 64 m cell as a pure function of the world, the species registry and the seed, and knows nothing about regions being wanted or drawn. `strata-physics` reads it for tree colliders (the server's trees) and `strata-foliage` (Phase 5b, a render crate above terrain, since it reads the terrain's tint and normal at a blade's root) draws it.

`strata-physics` (Phase 4a) sits above `strata-sim` and `strata-stream` and below the render crates, and the headless facade includes it: the collider is the server's ground. `strata-net-replicon` (Phase 4b) sits above `strata-physics` (the server spawns a `CharacterController` per client) and below the render crates; it is behind the facade's `net-replicon` feature rather than in the headless group, so a build that neither serves nor connects carries no transport, and `just server-check` holds `apps/server`, which turns the feature on, to a graph with no render, window, UI or text crate.

Below the render layer the allowed Bevy crates are `bevy_app`, `bevy_ecs`, `bevy_math`, `bevy_state`, `bevy_time`, `bevy_transform` and `bevy_tasks` (plus the umbrella's always-on support crates, and avian's `bevy_log`, `bevy_heavy` and `bevy_transform_interpolation`); `just server-check` asserts exactly that list on the headless facade graph and builds and tests the crates below the render layer on their own, so feature unification from the app cannot hide a render-side use in them.

### 3.2 bevy-slotted

Used now: `slotted-registry` (namespaced registries, data stage, freeze, tags) for biomes and materials, and since Phase 4c the `slotted` facade in `strata-client` (screens, the glass theme, HUD layers, the menu templates with settings persistence) and `slotted-test` for the client's headless screen tests. Reserved for later, with the boundary kept clean: `slotted-packs` as the asset source once mods exist, `slotted-script` for scripted content.

The dependency is a git dependency pinned to a rev in `Cargo.toml`, so a clean clone builds, and `.cargo/config.toml` carries a `[patch]` to `../../bevy-crates/bevy-slotted` for local development. Bevy versions must match exactly; both are on 0.19.1.

### 3.3 Feature flags on the facade

| Feature | Pulls in | Default |
|---|---|---|
| `client` | terrain, atmosphere, client, `bevy_render` and friends | yes |
| `headless` | core, registry, worldgen, sim, stream, physics, net port | always |
| `dev` | wireframe toggle, time-of-day scrub, the frame statistics log | yes in the slice |
| `ui` | the slotted UI in `strata-client`: glass theme, pause and settings screens, HUD layers (debug statistics, reticle, hotbar) | yes in the slice |
| `net-replicon` | the bevy_replicon adapter for the net port: a renet UDP server and client, interest by region, prediction. Both halves of replicon at once (the server binary carries the client's prediction types and the game the server's); a `net-replicon-server` / `net-replicon-client` split is a contained change for when it matters | no (`apps/server` and `apps/game` turn it on) |

`just server-check` asserts the `--no-default-features --features headless` graph holds only the Bevy crates section 3.1 allows below the render layer (so no `bevy_render`, `bevy_winit`, `bevy_ui` or `bevy_pbr`), the same kind of assertion slotted makes.

## 4. The basic loop

### 4.1 States and schedules

```
AppState:  Boot ──► LoadingRegistries ──► InWorld
```

- `Boot`: plugins register, the seed and launch options become resources.
- `LoadingRegistries`: `.biome.ron` and material definitions load from the asset directory with plain `std::fs` on the compute pool (`WorldLoadPlugin` in `strata-sim`; deliberately not Bevy's asset server, so a dedicated server needs no `AssetServer`) into `slotted-registry` stores, patch rounds apply, registries freeze. The `WorldSource` is built from the frozen registries and the seed. Nothing samples the world before this.
- `InWorld`: the loop below runs.

Schedules and sets, in order:

```
PreUpdate
  replicon receives (net-replicon): a client adopts its player, copies the clock; a server queues intents

FixedUpdate (SimTick, 20 Hz)
  SimSet::Input      previous positions recorded; a server dequeues one wire intent per player;
                     PlayerIntent consumed into PlayerCommand (the jump edge cleared)
                     a client sends the consumed command, numbered, after this set
  SimSet::Step       the clock and time of day; avian's PhysicsSystems (strata-physics):
                     PhysicsSet::Sync (WorldPosition -> Position), the collision step,
                     PhysicsSet::Character (move and slide, gravity, slopes, water),
                     PhysicsSet::Writeback (Position -> WorldPosition for dynamic bodies)
  SimSet::Observe    Observers refreshed; a client reconciles its prediction with ServerPosition;
                     a server publishes the clock carrier

Update
  StreamSet::Select  quadtree walk from each observer: wanted set of (RegionCoord, lod)
  StreamSet::Diff    wanted vs loaded: emit Load / Unload / ChangeLod, under a per-frame budget
  TerrainSet::Dispatch  spawn sampling+meshing tasks for Load, cancel for Unload
  TerrainSet::Collect   poll finished tasks, insert Mesh + Material, despawn unloaded
  TerrainColliderSet::{Dispatch, Collect}  the same for the lod 0 heightfield colliders
  FloraSet::{Dispatch, Collect}  (Phase 5) placement per region on the compute pool after RegionReady,
                        never gating it; tree colliders in the physics crate, grass buffers and tree
                        LOD entities in the foliage crate, all dropped on RegionUnload
  ClientSet::Camera     steering (flycam, orbit, WASD into PlayerIntent; stands down under a screen),
                        scripted flight, new player capsules (local and remote)
  ClientSet::Hud        the frame statistics, the slotted HUD layers, the debug keys
  the slotted UI        screens, focus, the settings store; the port's NetStatus published

PostUpdate
  OriginSet::Observe    observers refreshed again (the camera moved in Update)
  OriginSet::Rebase     if the primary observer is > threshold from RenderOrigin, move the origin,
                        rewrite every rendered Transform and every physics Position in one pass
  OriginSet::Sync       Transform.translation = (world - origin) as f32
  ClientSet::Follow     the orbit camera and the player capsules placed from the position
                        interpolated between the last two ticks
  a server decides per client what is in its interest, then replicon sends
```

`SimTick` is its own fixed schedule with its own clock so a dedicated server runs it without a window and the client can later run it ahead for prediction. Rendering interpolates between the last two sim states; the flycam is a client-side entity and does not go through the sim, the player does.

### 4.2 Coordinates

- `WorldPos(DVec3)`: metres, `f64`, `y` up. The only position type in sim and worldgen.
- `RegionCoord { x: i32, z: i32, lod: u8 }`: a square of the world at one LOD; size `base * 2^lod`.
- `RenderOrigin(WorldPos)`: a resource. A rendered entity's `Transform.translation` is `(world - origin) as f32`.

ADR 0002 has the numbers and the alternatives.

### 4.3 Terrain

A quadtree over the heightfield, a fixed vertex grid per node, distance-based LOD selection with skirts to hide cracks. Meshing is asynchronous on the compute pool and cancelled when a region leaves the wanted set. ADR 0003 has the design and what was rejected.

### 4.4 World generation

`strata-worldgen` is a stack of pure stages:

1. **Continents**: low-frequency domain-warped noise gives land/sea and broad elevation.
2. **Climate**: temperature (latitude band + altitude lapse + noise) and moisture (noise, coastal distance).
3. **Biome**: a lookup of (temperature, moisture, elevation) into the frozen biome registry, with a blend weight for the two nearest so borders are not hard lines.
4. **Relief**: each biome contributes a height function (ridged for mountains, billow for dunes, gentle fbm for plains) blended by weight.
5. **Surface**: material index per biome and slope.
6. **Flora** (Phase 5, `strata-flora`, not a stage of the sample but a function over it): per 64 m cell, species from the registry thresholded against the sample by a per-slot hash, big things first with exclusion, grass last.

`fn sample(&self, x: f64, z: f64) -> Sample { height, biome: [(BiomeId, f32); 2], material, ... }` and a batched `sample_grid` that terrain meshing uses. Biomes are `.biome.ron` files, so tuning is data and, later, moddable.

## 5. Phases

Each phase ends with `just ci` green, and the exit criterion is something a person can see or a test can assert.

### Phase 0: skeleton
Workspace, every crate as an empty library with its plugin, the facade, `apps/game` opening a window with a flycam over a flat plane, `.cargo/config.toml` patch to the local slotted checkout, `justfile` with `check`, `test`, `lint`, `fmt-check`, `server-check`, `run`. Exit: window opens, `server-check` passes, CI configured.

### Phase 1: worldgen
`strata-core` coordinate types with tests; `strata-registry` with the biome and material registries loading from RON via `slotted-registry`; `strata-worldgen` with the stack in 4.4, at least six biomes (ocean, coast, plains, forest, desert, mountain, tundra); `apps/worldmap` rendering a seed to PNG. Exit: the map looks like a world, `sample` is deterministic across runs and threads (proptest), snapshot tests on a few coordinates (insta).

### Phase 2: streaming and meshing
`strata-stream` quadtree selection and diffing with a budget, headless tests over synthetic observer paths; `strata-terrain` async meshing with skirts, vertex-colour biomes, unload; `RenderOrigin` rebasing. Exit: fly across the world at 200 m/s with no hitches, no cracks, no precision jitter 50 km from spawn, draw distance to the horizon.

Done 2026-09-11. Measured with `game --flight` (release, 2560x1440, vsync off, RTX 4090): a 60 s, 12 km flight at 200 m/s from (20 km, -5 km) into the mountains ran at 310 to 330 fps (3.1 ms average frame) with about 780 regions loaded, 8 meshes at most in flight, and no frame over 50 ms after the first second (the first drawn terrain frame compiles the material pipeline and costs 250 ms once); a 40 s flight from (50 km, 50 km) showed no jitter after rebasing. Skirts hide the LOD cracks; a coarse node's skirt wall shows as a thin bright dash at the foot of distant mountains where its straight edge rises above a finer neighbour, the artefact ADR 0003 accepted and the reason LOD morphing stays a Phase 3 candidate.

### Phase 3: look and feel
Terrain material with a texture array and slope/height splatting from `Sample.material`; `strata-atmosphere` with Bevy's `Atmosphere`, sun bound to sim time of day, distance fog; a water plane at sea level; debug HUD. Exit: the slice as described in section 1 at 60 fps on the dev machine at 1440p.

Done 2026-09-11. What landed:

- The terrain material's layers come from the frozen material registry, in registry order, so the mesh and the texture arrays can no longer disagree (they did: the first Phase 3 cut painted deserts with the snow set). Every detail read is stochastic (a random offset per vertex of a triangular lattice a quarter repeat wide, the three vertices of the triangle blended with the contrast renormalised about the layer's mean colour), which is what finally removed the grid of repeats the review found in the first cut; the detail fades into the layer's mean colour by 1.2 km under a slow albedo variation, and the biome tint takes over completely from 400 m to 6 km, so the horizon is a map of biomes and no texture is asked to survive ten kilometres. Slopes turn into the biome's own slope material from the biome's own threshold, darkened towards the tint at range so a range's rock faces still contrast with its snow. Normal maps fade out by 400 m, which is what stops the shimmer at grazing angles. Texture UVs are wrapped to a 5040 m period per region so `f32` precision holds anywhere in the world.
- LOD morphing (the CDLOD part ADR 0003 deferred): every vertex carries where it sits on the parent LOD's grid and the vertex shader slides it there with distance, in the shadow pass too, so a fine node meets its coarse neighbour exactly and a LOD swap changes nothing on screen. The skirt walls that showed as bright dashes in Phase 2 are gone from the stills; skirts remain as a belt for the frame between a swap and its replacement.
- The atmosphere reads the sim's `TimeOfDay` (the sim owns `DayLength` and `DayStart`); Earth's medium at 55 percent density, EV100 14.2, four shadow cascades to 20 km on a 4096 map so ranges cast shadows at dusk, the planet's bare ground sea-coloured. The planet is an entity with a `WorldPosition` one radius below sea level under the observer, so a render-origin rebase never moves the modelled surface (the review caught the camera reading as altitude zero after the first rebase). The sea is a plane at sea level that follows the observer, 400 km across so it meets the sky past the planet's horizon, with a depth-dependent opacity read from the camera's depth prepass: the shore fades in from nothing over a couple of metres and deep water hides its floor.
- Debug HUD in `bevy_ui` (F3), wireframe (F4), time-of-day scrub (`[` and `]`), all behind the facade's `dev` feature. Streaming now reaches 48 km with 32 km roots, past the sea horizon from flight height.

Measured with `game --flight` (release, 2560x1374 client area of a 2560x1440 window, vsync off, RTX 4090, Ryzen host): the 60 s, 12 km flight at 200 m/s from (20 km, -5 km) into the range ran at 200 to 241 fps per half-second window (213 mean, 4.6 to 5.0 ms average frame) with 824 to 922 regions loaded; the worst frame after the first second was 39.9 ms, and the first drawn terrain frame still compiles the pipelines and costs 250 ms once. A 45 s flight along the coast from (4 km, 21 km) ran at 196 to 243 fps. Shadows cost about a millisecond: the same flight without them runs at 270 fps. `docs/research/slice-*.png` are frames from those flights.

### Phase 4: a player in a served world

Assets (scatter meshes, `slotted-packs` as a layered asset source, hand-made props) are deferred to Phase 5 so this phase stays about the loop, not the content. Versions: `avian3d` 0.7, `bevy_replicon` 0.44 with `bevy_replicon_renet` 0.20, all on Bevy 0.19.

**4a: character through the sim.** A `Player` entity in `strata-sim` with `WorldPosition`, driven by `PlayerIntent` (move vector, jump, look yaw) applied in `SimSet::Input` and stepped in `SimSet::Step`. Physics is `avian3d` in a new `strata-physics` crate above sim and stream: a heightfield collider per lod 0 region built from the same samples as the mesh (the collider is the authoritative ground, so the server has it without a renderer), a kinematic character controller with ground snapping, gravity and slopes. The player is the primary observer. Client: a third-person orbit camera following the player, interpolated between sim ticks; the flycam stays behind a flag. Exit: walk from spawn up a mountain and into the sea at 20 Hz sim with a smooth 200 fps camera, headless tests that a player dropped from 100 m lands on the sampled height.

Done 2026-09-12. What landed:

- `strata-sim`: `Player { id: PlayerId }` (requires `WorldObserver`, so a server streams around every player), `LocalPlayer` (requires `PrimaryObserver`: the render origin follows the player now, as it followed the flycam), `PlayerIntent { move_dir, yaw, jump, sprint }` written by input or a script at any rate and consumed once per tick in `SimSet::Input` into `PlayerCommand`, which is what the tick acts on; the jump is an edge and is cleared on consumption so a press between two ticks counts once. `PreviousWorldPosition` is recorded at the start of every tick for the client's interpolation. `ScriptedIntent` drives the headless walker and `--auto-walk`.
- `strata-physics` (avian3d 0.7, `3d`, `f32`, `parry-f32`, `parallel`, no debug renderer): `PhysicsPlugins::new(SimTick)` with the five `PhysicsSystems` sets inside `SimSet::Step`, so physics steps exactly once per 20 Hz tick on the fixed clock. Avian runs in render space in `f32`: its `Position` is derived from `WorldPosition` at the start of every tick and avian's own `Transform` sync is switched off, since the sim's origin sync owns every `Transform` (ADR 0002). On `RenderOriginChanged` every body's `Position` is rewritten the same frame the transforms are. A physics entity needs a `Transform` because avian's collider hook reads the entity's `GlobalTransform` scale on insert and scales the shape to zero without one; the transform's translation is the sim's to write, as everywhere. Every lod 0 `RegionLoad` gets a sampling task on the compute pool and then a static body with a parry heightfield at the region's centre; the heights are sampled at exactly the mesh grid's points and parry splits cells on the mesh's diagonal, and a facade test holds the two crates to it bit for bit on four regions of seed 42. The character is a kinematic capsule (0.35 m radius, 1.7 m tall) moved by avian's `MoveAndSlide` with the rules a walker needs: gravity 15 m/s² and a 4.5 m/s jump, a 50 degree slope limit that flattens steeper ground into a wall while grounded, a 0.5 m step up, a snap down so walking downhill keeps contact, and water (below sea level the speed halves; a metre under it buoyancy holds the capsule's centre at the float line). A character over no collider at all holds still until the ground streams in, which covers spawning ahead of the collider and a teleport. Walk 5 m/s, sprint 9 m/s.
- `strata-client`: WASD relative to the camera's yaw, space and shift, into `PlayerIntent`; a capsule visual per player and an orbit camera (right mouse to look, scroll for 2 to 20 m), both placed in `PostUpdate` after the origin sync from the position interpolated between the last two ticks by `Time<Fixed>::overstep_fraction`, the camera kept half a metre above the sampled ground and the sea under it and under the middle of its boom. The camera is a plain render-space entity and not an observer. The flycam is behind `--flycam` (and `--flight`); `--auto-walk` walks the character along `--look`.
- `game --headless` now spawns a scripted walker instead of a static observer: over 900 frames it walked 70 m north, grounded throughout, with regions loading and unloading around it (820 to 826) and 128 to 132 colliders under it, and the headless graph (`just server-check`) holds only the crates ADR 0001 allows plus avian's `bevy_log`, `bevy_heavy` and `bevy_transform_interpolation`, none of them a renderer.

Headless tests (`crates/strata-physics/tests/headless.rs`, synthetic worlds, no GPU): a player dropped from 100 m lands within 0.2 m of the sampled height; a player walking 10 s over hills stays within 0.35 m of the ground and grounded every tick; a 30 m wall stops a walker short of it; a jump leaves the ground and comes back; a walker into a 5 m deep sea floats with its centre a metre under the surface; a rebase (a player 5 km out, then teleported 3 km further) moves every collider's `Position` by exactly the origin delta the same frame; colliders unload with their regions.

Measured with `game --auto-walk` (release, 2560x1374 client area, vsync off, RTX 4090): 183 to 237 fps per two-second window (4.2 to 5.8 ms average frame) with 817 to 835 regions loaded while the character walked 150 m from the default spawn into the sea at 5 m/s on land and 2.5 m/s swimming; the worst frame after the first second was 37.8 ms (the terrain pipeline compile on the first drawn frame costs 250 ms once, as before). The scratchpad stills show the capsule standing on sand with its shadow, the camera behind and above, the sea reached and the capsule afloat a metre under the surface.

**4b: headless server and replication.** `apps/server` runs `StrataPlugins::headless()` with a listen socket; `apps/game --connect host:port` runs the client. `strata-net` keeps the port; `strata-net-replicon` is the adapter: `WorldPosition`, `Player` and `SimClock` replicated, `PlayerIntent` sent client to server as a replicon event, the server owns the sim and the client predicts its own player then reconciles. Interest management is the region grid: a client sees entities in regions within its stream radius. Exit: two clients on one local server see each other move, the server streams and unloads regions around every connected player with no window, and `just server-check` still passes.

Done 2026-09-12. What landed:

- `strata-net` grew `NetRole { Standalone, Server, Client }` and a `NetPlugin` that only records it (in the headless group, `Standalone` unless an adapter says otherwise), so the port stays the vocabulary: game code marks a `Replicated` entity and reads the role, and a build without the adapter is the local sim it always was.
- `strata-net-replicon` (bevy_replicon 0.44, bevy_replicon_renet 0.20, renet's netcode over UDP, default features off so nothing draws): `ReplicationPlugin { role }`. The server listens once the world exists (no client can connect before there is ground), spawns a `Player` with a `CharacterController` for every client that passes replicon's protocol check, two metres apart on a sunflower spread because two capsules on one spot are each other's wall (the first slot nobody is standing within a metre of, so a slot freed by a leaver is reused and an occupied one is not), a third of a metre over the ground so the first step stands it there, and despawns it when the client goes. The player's id is the client's network id, so a client recognises its own player the frame it arrives; a `Welcome` event confirms it and names the server's seed, and a client that loaded another seed is told so in its log. `WorldPosition` (`f64`, as is), `Player` and the sim clock are replicated; the clock rides on a singleton entity with the day settings, because replicon 0.44 replicates a resource only onto a client that does not already hold it and every client holds a `SimClock` from `SimPlugin`. Replicon ticks in `FixedPostUpdate`, so the server sends at the sim's 20 Hz.
- Intents: the client sends the `PlayerCommand` its own tick consumed (so the jump edge the local tick saw is the one the server sees), numbered, once per tick over a reliable ordered channel. The server queues them per player and its `SimSet::Input` dequeues one per tick into `PlayerIntent` before the sim consumes it, so the server runs the same sequence of commands the client predicted with; an empty queue holds the player still and leaves the ack where it was (so the position reported for a sequence is always the one that sequence produced; running the last intent on would move the player under a still ack, which the client can only read as a wrong prediction), a queue past three entries drops the oldest at arrival with their jumps folded forward, a sequence that does not advance is ignored, and `AppliedInput` on the player echoes the sequence the tick used. Direction clamped, non-finite dropped; the physics decides the rest.
- Prediction: the client's own player is a `LocalPlayer` with a `CharacterController`, so the same physics moves it at once, and received positions go into `ServerPosition` (a replicon receive marker) instead of over the prediction. After each tick the client records the position it predicted for the sequence it sent; when the server's position for an acked sequence is more than 0.5 m from the prediction for that same sequence, the player and the predictions still on record are shifted by the difference (a shift, not a set: setting the player to a position several ticks old rewinds those ticks and the next ack disagrees by exactly that, a cascade the review of the first version measured at one correction every 200 ms while walking). That is exact rather than latency-guessing, and it is a correction, not a reconciliation: rewinding to the acked state and replaying the later intents needs the character step callable outside the schedule, noted in section 6. Remote players carry no controller and are drawn from the server's positions interpolated by the same `PreviousWorldPosition` as the local one; `strata-client`'s `RemotePlayerPlugin` gives them a capsule, and the client removes their `WorldObserver` so the terrain streams around its own player only.
- Interest: the server runs the streamer's own selection per client (a `Selector`, with its hysteresis, walked again when the player has moved eight metres) and an entity with a position is visible to a client when the selected region containing it is at LOD 2 or finer, about 1.3 km with the default streamer; a client's own player is always visible. The decision is a pure function (`in_interest`) and the bit is set through replicon's `ClientVisibility` before replication sends, so a client authorized this frame never receives an entity outside its interest. Out of interest the entity is despawned on the client; back in, it is respawned.
- `apps/server`: clap `--seed`, `--port`, `--tick-log`, `--frames`, `--assets`; `MinimalPlugins` at 60 Hz, the log plugin, the headless facade with `net-replicon`, `ReplicationPlugin::server`. A status line every two seconds: tick, each client's player position, grounded flag, round trip and interest size, regions and colliders. `just server-check` now also asserts the server binary's graph carries no render, window, UI or text crate (it holds bevy_app, ecs, time, log, state, transform, tasks, replicon and renet) and tests `strata-net-replicon` standalone. `just run-server` and `just run-client`.

Tests: `crates/strata-net-replicon/tests/two_clients.rs` wires a server and two clients in one process through replicon's test backend over a flat synthetic world: both clients get a player and see the other's; the local one has a controller and the remote one has neither controller nor observer; the clock arrives; client 1 walks north for two seconds and the server moves its player 5 to 12 m while client 1's prediction stays within a metre of it and client 2 sees the remote player walk; player 2 teleported five kilometres away disappears from client 1 (and player 1 from client 2) and reappears when brought back; a client that disconnects takes its player with it everywhere; and a client that wanders off without the server (30 m) is corrected back within 0.5 m. Two tests added by the Phase 4 review sprint a client and stall the exchange: two ticks of stall cost no correction and the queue's lag stays under two metres, six ticks cost exactly one, and a 30 m error while sprinting costs exactly one rather than one every few ticks. Unit tests in `server.rs` cover the receive path (an intent reaches only the sender's player, a non-finite one is dropped and an over-unit direction clamped, a sequence that does not advance is ignored, a flood is trimmed on arrival with its jump kept, an empty queue holds the player and the ack) and the free spawn slot; the interest decision and the per-client selection have theirs.

Measured 2026-09-12 (release, localhost, RTX 4090): `server --port 5000` in the background, `game --connect 127.0.0.1:5000 --auto-walk --frames 3000` and, five seconds later, a second client for 900 frames. The server log shows both clients joining, both players walking north at 5 m/s grounded throughout (player 1 from z -252 to -362 over 22 s), round trips of 17 to 39 ms as renet measures them, 814 to 832 selected regions per client, 823 to 832 regions loaded and 128 to 148 colliders built around both, the second client leaving cleanly, and, once both had gone, the loaded regions falling to zero. No warning or panic in any of the three logs. Client 2's stills (`scratchpad/server/client2-600.png`, `client2-900.png`) show its own capsule on the sand and client 1's capsule thirty metres ahead, further along between the two frames; client 1 tagged the second player remote in its log. Client 1 ran at 195 to 201 fps alone and 57 to 60 fps while the second window shared the display, then 200 fps again after it closed.

**4c: first slotted screens.** The `slotted` facade in `strata-client`: a pause menu from `slotted-menu` templates (resume, settings with persistence, quit), and the debug stats moved onto a slotted HUD layer with the glass theme. Exit: Escape opens the pause menu over the world, settings persist across runs, the headless slotted test harness drives the menu in a test.

Done 2026-09-12. What landed:

- `strata-client` gains a `ui` feature (on by default, passed through the facade's `ui` feature; the facade's `client` alone is a renderer with no screens) that adds `SlottedPlugins::default()` (features `ui` and `menu` only: no Lua runtime, no packs, no browser, no GPU icon bake) to `ClientPlugin` when the app has Bevy's UI plugins, plus the two navigation plugins `DefaultPlugins` leaves out. The glass theme and the two OFL font families it names are copied into `assets/themes/` and `assets/fonts/` with their licences, as slotted's examples do (`assets/README.md`). The `slotted` and `slotted-test` git dependencies share the registry crate's rev and its `.cargo/config.toml` patch.
- The pause screen is the crate's `slotted:pause` template as it is: Escape pushes it, Escape or Resume pops it, Settings pushes the game's generated settings screen, Quit asks with a danger confirm and then exits. What the game adds is a `MenuConfig`, the English for its own keys, the quit flow, and the input gate: while any page or modal is on the stack the cursor is released and shown, the steering systems (flycam, orbit, WASD into `PlayerIntent`) do not run and the local player's intent is cleared, so the menu's keys never also walk the character. The simulation does not pause; this is an MMO and the world goes on under the menu (the test asserts the tick count keeps climbing).
- Settings are a `SettingsSpec` (field of view 40 to 100 degrees, a HUD toggle, mouse sensitivity 0.2x to 3x, a master volume placeholder with a note that nothing plays yet) that `slotted-menu` turns into the screen, the store, the rules and the file: `<platform config dir>/sci-fi-scrolls/settings.ron` (`~/.config` on Linux), or the `SettingsFile` resource an app inserts first. Applying is done by reading the store every frame and writing only a difference: the field of view onto every orbit or fly camera's `Projection`, the sensitivity onto both cameras (`0.002 rad/px` times the factor), the HUD toggle onto the reticle and hotbar layers. No camera code changed for it: `OrbitCamera::sensitivity` and `FlyCam::sensitivity` were already fields.
- The HUD is three slotted layers that take the built-in ids so a mod's `insert_above("hotbar", ..)` means what it says: `debug` (the frame statistics in a `hud.panel` top left, rebuilt four times a second through a `HudUpdate`, F3 hides it), `crosshair` (the plugin's own reticle) and `hotbar` (an eight by one slot grid over an `Inventory` entity behind a menu with no screen, hidden while a screen is open). The `bevy_ui` HUD is gone. The inventory model exists from now on: the first item that lands in that inventory shows in the hotbar.
- `game --open-pause` pushes the pause screen two seconds in, for a screenshot; the scratchpad stills show the statistics, the reticle and the hotbar over the spawn, and the pause modal readable over the scrimmed world with the hotbar and the reticle standing down under it.

Headless tests (`crates/strata-client/tests/ui.rs`, `slotted-test`'s harness over `SimPlugin`, `StreamPlugin` and `ClientPlugin`, no window, no GPU): a synthetic Escape opens the pause and clears a walking player's intent; the Settings button found by role and tag opens the generated screen; the field of view slider dragged to its end lands on the camera's projection and, once the change settles, in a settings file under the temporary directory; a second app over the same file starts from the saved value; Escape twice returns the keys to the game with the sim clock further on than before; Quit asks and an accepted confirm writes `AppExit`; the three layers are spawned and visible, the statistics text carries the frame numbers, F3 hides the statistics alone, the settings toggle hides the reticle and the hotbar alone, and a screen hides those two while the statistics stay.

**Integration.** Done 2026-09-12, once the three sub-phases met in one tree:

- The port gained `NetStatus` (connected, round trip, players in sight, prediction corrections), which the adapter publishes on both ends every frame (written only on change) and the HUD reads next to `NetRole`; the statistics layer now has a network line (`net standalone (local sim)`, `net client rtt 12 ms players in sight 2 corrections 1`, `net client connecting`). The HUD sits in a render crate and the adapter is behind a feature it cannot see, so the port is the only way the two can talk, which is what a port is for.
- The other players' capsules are `ClientPlugin`'s own system now (after the local capsules, so the two never race), so `--connect` adds the adapter and nothing else; `RemotePlayerPlugin` is gone.
- The pause menu on a connected client: the screen stack clears the local `PlayerIntent`, the next tick consumes that into the `PlayerCommand`, and the command is what `send_intent` ships, so the server's copy of the player halts one round trip later. The UI test asserts the consumed command, not just the intent, goes to zero under the menu; a connected client with `--open-pause` shows the modal over the served world with the statistics (and the network line) still readable.
- slotted's item preview loads five `icons/hint_*.png` at startup that the Phase 4c copy of the theme did not bring; every client run logged five missing-asset errors. Copied, with `assets/README.md` saying from where. The remaining startup warnings (a gamepad without a mapping, one outdated swap chain texture on the first frame) are Bevy's and not ours.
- The server's status line carries what a simulation tick cost (mean and worst over the window, measured around `SimTick`), so a server budget has a number to be written against.
- ADR 0004 records the networking decisions: replicon behind the port, the region grid as interest management, the snap-based prediction and what real reconciliation needs, and why the world does not pause under the menu. README and CLAUDE.md describe the server, the flags, the controls and the new crates.

Measured 2026-09-12 (release, 2560x1374 client area, vsync off, RTX 4090, localhost): standalone, the character at the spawn with the statistics, the reticle and the hotbar over it, 195 fps (5.1 ms) with 814 regions loaded at frame 400 (`--auto-walk` measured 183 to 237 fps over 150 m in Phase 4a). Connected: `server --port 5000`, then `game --connect --auto-walk --frames 3000`, then five seconds later a second client for 900 frames. Client 1 ran at 192 to 198 fps (5.0 to 5.2 ms) alone and 52 to 60 fps while the second window shared the display (the compositor's rate, as before), client 2 at 116 to 132 fps for its 900 frames; round trips 11 to 19 ms as renet measures them; no warning or error in the server log. Client 2's stills show its own capsule and client 1's warm-tinted one thirty metres ahead, further along between frames 600 and 900. The server ticked at exactly 20 Hz (40 ticks per 120 frames, every status line): a tick cost 0.18 to 0.28 ms mean with nobody connected and nothing loaded, 0.40 to 0.71 ms mean with two clients walking (823 to 832 regions selected and loaded around them, 128 to 140 colliders), worst tick 0.9 to 1.1 ms in every window but the one in which the second client joined (3.6 ms). Regions and colliders fall to zero after the last client leaves. In that run every connected client recorded one prediction correction at spawn: the server dropped the player two metres onto ground it had, while the client's own collider for that region was still being built, so the client's character held still over no ground for a few ticks while the server's landed; the first acked position was then over half a metre from the prediction. The review's fixes below spawn the served player a third of a metre up instead. The frame figures above are for an unfocused window with no other window created or destroyed during the measured window: a window mapped or destroyed on the same display costs the game one 80 to 106 ms frame (the review reproduced it with a blank foreign X window next to a standalone run), a screenshot capture costs 30 to 40 ms on its frame, and the window that holds X focus renders at the compositor's 60 fps; none of it is Phase 4 code, and worst-frame figures exclude the screenshot frames.

**Review fixes** (2026-09-12, the Phase 4 code review). The ack was not exact: with an empty intent queue the server ran the last intent on while `AppliedInput` stood still, so ordinary jitter (and any lost packet on the reliable channel, a 300 ms stall) made the client compare a moving server position against one prediction and correct itself; the server now holds the player still through a stall and trims the queue at arrival. The correction was a set, not a shift: it put the player at the server's position for a sequence several ticks old and cleared the history, so one correction while walking cascaded into one every 200 ms (sixty in a twelve-second walk, the HUD's `corrections` counter meaningless); the client now shifts the player and its history by the error. Both are pinned by `two_clients.rs` tests that fail on the old code (8 and 20 corrections respectively where 0 and 1 are asserted). The served player spawns a third of a metre over the ground and on the first free sunflower slot rather than the slot indexed by the live player count. A connecting client no longer logs `disconnected` before it has connected. The settings screen is registered as a page, so the pause modal is hidden under it instead of showing through the glass panel (the harness test asserts it). The physics writeback for dynamic bodies was dead: the post-step sync that keeps a character's `Position` current ran over every body and put a falling body back where its stale `WorldPosition` said, before the writeback could run; a test that drops a ball into a bowl caught it, and the post-step sync now covers characters only. The rebase test steps the character on after the rebase (it lands on the sampled height and walks off grounded). `strata-physics` no longer lists `strata-registry` as a normal dependency (test-only). The adapter still turns on both the client and the server halves of replicon (a feature leak, not a layering breach); splitting it into `net-replicon-server` and `net-replicon-client` facade features is a contained change left for when a server build's size matters.

**Deferred:** vegetation became Phase 5 below; rocks and props, `slotted-packs` as the asset source and the first interior cell go to Phase 6.

### Phase 5: vegetation

Grass and trees over the whole world, cheap enough to keep the slice's frame rate, animated by wind, present on the server as colliders, and defined as species data so an alien biome is a file and not a shader. Decided in ADR 0005 from the discovery board (`docs/research/foliage-moodboard.md`) and the disposable spike (`docs/research/foliage-poc.md`, branch `spike/foliage-poc`, mined and not merged). Props, `slotted-packs` and interiors stay deferred to Phase 6; the spike's flags and its render-side placement do not carry over.

The spike's numbers set the budget: at 1080p, 530 k blades and 2.5 k trees around the character cost 3.3 ms over the bare slice once only the near tree LOD casts shadows, and the same scene cost 18 ms when every LOD cast. Grass through `Mesh` assets stalls a frame by about 65 ms per cell at speed; grass through raw buffers does not. Per-entity grass costs 45 ms of CPU and is not an option.

**5a: species and placement, below the render layer.** `strata-registry` gains a flora registry from `assets/flora/*.flora.ron`: a species is a kind (grass, bush, tree), the biomes it grows in with a weight, a density or spacing, slope and altitude and sea-distance limits, a size range, a tint pair, and for trees the generator's parameters (levels, branch factor, taper, split angle, card size, leaf palette, emissive) and a variant count. `strata-worldgen::Sample` gains the one scalar placement reads that the terrain does not (moisture, or a fertility derived from it), deterministic like the rest. New crate `strata-flora`, no Bevy: `place_cell(coord, source, flora, seed) -> CellFlora` over a 65 x 65 height sample of the cell (the grid the collider already samples), trees and bushes first from a jittered grid per species thresholded by a per-slot hash against the species' fit at that point with exclusion radii between them, grass last as a compact per-slot record (root offset, height, width, phase, stiffness, tint index) kept clear of trunks; `Rng` and hashing from `strata-core` only. Exit: proptest that two threads and two runs place the same bits for the same cell; insta snapshots of a few cells; a headless test that a tree never stands in the sea or on a cliff past the species' limit; `cargo test -p strata-flora` needs no Bevy.

Done 2026-09-12. What landed:

- `strata-registry` gains `flora.rs`: `FloraDef` (kind as a string like the relief kind, biome weights, spacing or density, slope, altitude, sea clearance, moisture window, size range, exclusion radius, dry and lush tints, an emissive colour, and `TreeParams` with defaults, absent when its height is zero because RON's `Value` has no `Option`), `FloraId`, and a `FloraFit` per species resolved at freeze into a dense weight per biome id. `flora/*.flora.ron` load with their own patch rounds; a missing directory is a world with nothing growing. Five species ship: oak, pine, scrub, meadow grass, dry grass.
- `strata-worldgen::Sample` gains `moisture`, the climate field the classifier already computed, so placement reads wetness without a second sample. The seed 42 snapshot grew a line per point and changed nothing else.
- `strata-flora` (no Bevy): `FloraTable::from_registries` copies what placement needs out of the frozen set once; `place_cell(coord, source, flora, seed, scratch)` samples the cell's 65 x 65 metre grid (the collider's grid), places trees then bushes on jittered grids at their spacing, keeping a slot when its hash falls under the species' fit at that point (biome weights by the sample's shares, zero outside the moisture, altitude, slope and sea limits) and nothing placed is within the larger exclusion radius, then grass per species at its density, clear of trunks, as a 16-byte `GrassBlade` record (root as millimetre `u16`s, height bits, blade height, phase, stiffness, species, lushness, shade, ground normal) ready for a GPU buffer. Randomness is `mix64` of the seed's `flora` derivation, the cell and the slot; the arithmetic is `f32` adds, multiplies and square roots.
- Tests over the shipped assets: proptest that two tables from one seed place the same bits, a parallel sweep equal to the serial one bit for bit, every plant inside its species' limits and outside every other's exclusion radius, no blade in the sea, the spawn's coast growing scrub and grass and no tree, the first forest cell within 50 km growing trees, and an insta snapshot of one cell. `just server-check` builds and tests the crate on its own.

Measured (release, one thread): 2.9 ms per cell, 1.7 ms of it the world sampling, for 23 plants and 8 400 blades on the spawn's coast. On the compute pool that is a cell per frame per thread with room to spare.

**5b: grass on the client.** New render crate `strata-foliage`: after `RegionReady` on a lod 0 region, a compute-pool task runs `place_cell` for the region and the collector attaches, per cell, one grass entity with the shared blade mesh for the near LOD and a material instance whose storage buffer holds the cell's grass records; a second entity with the mid mesh (fewer blades, fewer vertices per blade) under a `VisibilityRange` that crossfades from the near one and fades out where the terrain's tint takes over. The vertex shader pulls its record by `vertex_index / vertices_per_blade`, places the blade on the sampled ground with the terrain normal and tint at the root, bends it by the wind (5d) and collapses empty slots. The material is an extension of `StandardMaterial` with diffuse transmission, drawn double sided, no shadow casting, through the terrain's depth prepass. Exit: grass to 150 m around the character at the spike's density (about 6 blades per square metre) with the mid band to 300 m, a 200 m/s flight over it with no frame over 40 ms after the first second, and the character's `--auto-walk` under 8 ms at 1440p with grass and the Phase 4 scene.

Done 2026-09-12. What landed:

- `strata-foliage` (render crate above terrain): `FoliagePlugin` answers `RegionReady` at lod 0 and 1 with a compute-pool task that runs `place_cell` for the region's cells and attaches, under the region entity, one grass entity per cell and LOD: a shared blade mesh from a size bucket (2 k to 32 k slots, seven vertices a blade near, one triangle per fourth blade mid) and a material instance whose storage buffer holds the cell's 16-byte records, uploaded once with `create_buffer_with_data`. The vertex shader takes its record by `vertex_index` (less the mesh's first vertex, since Bevy packs meshes into shared slabs), stands the blade on the record's root with the ground normal, gives it the species' width and dry-to-lush tint by the record's moisture, bends it in the wind and collapses slots past the buffer's end. Cells get an explicit `Aabb` (the shared mesh's own bounds are a unit blade), `NotShadowCaster`, and `VisibilityRange`s that crossfade near into mid at 110 to 130 m and fade mid out at 280 to 320 m; lod 1 regions carry the mid LOD alone. `strata-sim` gains a `Wind` resource (direction, strength, gust scale) that the plugin writes each frame, with the time, the previous time and the render origin wrapped to 4 km, into one storage buffer every cell's material shares, so no material is ever modified after creation. `game` gains `--no-foliage` and `--wind`.
- Two Bevy 0.19 facts cost most of the day and are recorded here so nobody pays again: a shader that imports a module by name (`#import strata::grass_common`) whose file the app never loaded waits forever with no error, so every shared module is loaded by the plugin and held; and WGSL refuses a storage pointer as a function parameter, which naga_oil reported only as "function is invalid" once the module was actually loaded. The wind is a buffer and not a material uniform because a material asset modified every frame re-prepares its bind group, and with a material per cell that is hundreds of bind groups a frame.
- The statistics line gains the GPU time of Bevy's timed passes (`RenderDiagnosticsPlugin`, which Bevy adds only under its Tracy feature): the main opaque pass, tonemapping, upscaling and the GPU preprocessing, not the prepass or the shadow passes, which Bevy does not time.

Measured 2026-09-12 (release, 1920 x 1080, RTX 4090). A 60 Hz display attached during the session capped every run at 16.7 ms wall clock, baseline included, so the GPU figure is the comparison: standing at the spawn looking inland, 319 cells and 2.5 M blade records drawn (the near LOD on 16 cells), the timed passes take 1.4 to 1.6 ms with or without grass; the 200 m/s flight over the plains (up to 544 cells and 7.3 M records, 4 to 7 ms per cell placed on the pool) holds 60 fps at the cap with the worst frame after the first second at 43 ms once during the initial fill and 32 to 38 ms after. The stills show meadow and dry grass on the spawn's coast, tinted by moisture, the mid band on the far slope.

**5c: trees on both ends.** `strata-flora` gains the tree generator: from a species' parameters and a variant seed, a skeleton (recursive branches, Weber and Penn reduced to what the file names) and from it three levels of geometry as plain vertex streams (bark tubes and leaf cards at full detail, fewer levels and larger cards reduced, two crossed cards far), Bevy-free so the server can read a trunk's radius and height from the same code. `strata-physics` answers lod 0 `RegionReady` with a static capsule collider per tree, dropped on unload, tested headless (a walker into a trunk stops; a tree's collider is where the client draws it, asserted by position). `strata-foliage` builds each species' variants into meshes once (bark opaque, leaves masked; alpha-to-coverage under MSAA) and spawns per region a tree entity per placement with three LOD children under `VisibilityRange`, near LOD casting shadows and the others `NotShadowCaster`; regions at lod 1 and 2 get trees only, at lod 3 and above nothing. The shadow cascades end where trees are cards. Exit: trees to 800 m, the spike's scene (2.5 k trees in view) under 4 ms over the bare slice at 1440p, no popping on LOD change that a still at the crossfade band shows, and a headless two-client test where a tree the server collides with is one both clients place.

Done 2026-09-12. What landed:

- `strata-flora::tree` (no Bevy): `build_tree(params, species_seed, variant)` grows a skeleton from the species' parameters (levels, children per branch, taper, split angle, trunk and card ratios) into plain vertex streams at three levels of detail: six-sided tubes with cards along the twigs, four-sided tubes with larger cards at two levels, and two crossed cards over a stub. `variant_height` and `trunk_collider` give a trunk's height and cylinder from the same numbers without building a mesh, so the server and the client agree on a trunk without either seeing the other's work. `Species` carries its `TreeParams` and a seed hashed from its id, so adding a species never reshapes another. `place_cell_with(.., grass)` leaves the grass out for a coarse region or a server.
- `strata-physics::flora`: the heightfield task of every lod 0 region also runs the placement over the samples it already has and returns the trunks; the collector spawns a static cylinder per trunk next to the ground body, tracked and despawned with the region. `FloraPlacement` holds the species table, built from the registries on first use or inserted by a test. The headless test builds a one-species table over a flat world, checks one cylinder per placed tree at the placed position with a ray landing on its top, and walks a character at the nearest trunk: the capsule never enters the trunk's radius, is deflected sideways by over half a metre and gets past, which is what a round trunk does to a walker.
- `strata-foliage::tree`: every species' variants become meshes once (bark and leaf cards per LOD, a procedural leaf-cluster alpha texture), under a `TreeMaterial` extension that shares the grass's wind buffer and bends by the two wind attributes. A placed tree is an entity per mesh of every LOD its region's distance can show, all at one transform and no parent: a lod 0 region carries all three, a lod 1 region the reduced and card LODs, a lod 2 region the cards alone. Only the near LOD casts shadows. That took the spawn forest from 592 k entities to 232 k for the same 84 k trees.

Measured 2026-09-12 (release, 1920 x 1080, RTX 4090, the 60 Hz cap of the morning gone): standing in the forest at (-2528, -37088) with 84 570 trees and 6.8 M blade records placed over 512 cells, 145 fps (6.9 ms) with the timed GPU passes at 1.2 ms; the 200 m/s flight over the forest at 95 to 106 fps (9.4 to 10.5 ms) with the worst frame after the first second at 43 ms, placing 300 to 400 cells a second on the pool at 2.5 to 4.8 ms each. The still shows oaks and pines to the horizon of the trees' range over the forest floor's grass, the near trees casting shadows across the player.

**5d: wind and shading.** `Wind` resource in `strata-sim` (direction, strength, gust scale; a client-side default, a server value once weather exists) into one uniform on the foliage material with time, previous time and the render origin wrapped to a period; the bend (height-squared lean along the wind with the Crysis renormalisation, per-plant phase and stiffness from the record or the vertex, leaf-edge flutter) in a shared shader module used by the main pass and the prepass, so shadows move and motion vectors are true; normals rebuilt after the bend. Leaf cards get canopy-sphere normals from the generator; blades take the ground normal; both take a hash tint and the species' tint pair by fertility. Exit: a still of a gust front visible as brightness across a field; `--aa taa` and the default MSAA both clean on the same scene; the debug HUD shows the foliage counters (cells, blades, trees, entities, build time).

Done 2026-09-12. What landed:

- The wind was already one resource and one buffer from 5b; 5d finished the shading. Leaf cards take canopy-sphere normals from the generator (seven tenths the direction from the crown's centre, three tenths the card's own facing), so a crown shades as one rounded mass; the tree bend rotates the normal with the branch (Rodrigues from the rest direction to the bent one); every tree mesh carries a `MeshTag` hashed from its place that the shader turns into a brightness and a warm-cool lean, so no two neighbours are the same green; grass carries the gust at its root into the fragment and brightens under it, so a front crosses a field as light and not only as motion. The HUD's statistics gain a foliage line (cells, blades, trees, entities, placements in flight) and the frame line the GPU time; `game --aa msaa4|off|taa` picks the anti-aliasing, TAA bringing its own prepasses and jitter.
- A second naga_oil rule joins the first: an imported symbol's name is renamed wherever it appears, struct fields included, so a field must not share a name with an imported function (`gust` became `gust_here`). Reserved words (`from`) are refused with a clear error, at least.

Measured 2026-09-12 (release, 1920 x 1080, RTX 4090): the forest at (-2528, -37088), 84 570 trees and 6.8 M blades placed, at 178 to 180 fps under 4x MSAA and 181 to 185 fps under TAA, the timed GPU passes at 1.2 to 1.4 ms either way; both stills clean, the TAA one softer on the far cards. The variance across runs today (145 to 180 fps on the same scene) is the machine, not the code: runs during a build or right after CI sit at the low end.

**Polish, 2026-09-12**, after a first look at 5a to 5d: textures and the far field.

- The far field: every terrain vertex carries a canopy colour and cover (`ATTRIBUTE_TERRAIN_COVER`), computed per biome at freeze time from the flora registry (each tree or bush species' lush tint weighted by its fit, its spacing and the crown of its mean height), and the terrain shader blends the ground towards it from 250 m to 800 m, inside the trees' own range, so a forest is a forest from ten kilometres and the trees fade in over ground already their colour. The far card LOD's normals face the sky, so the tree line no longer goes black edge-on, and the card fade runs 600 to 800 m.
- Streaming in: placements are collected nearest the observer first and stop for the frame at 2 500 entities spawned, and every foliage mesh carries its spawn time in the high half of its `MeshTag`, which the vertex shaders turn into a grow-in from the ground over the first second, in the prepass too, so a cell arriving is a growth and not a pop. The approach flight into the forest went from 175 hitches to 27.
- Textures: two Poly Haven bark sets (`bark_oak`, `bark_pine`, 1k, CC0, in `ATTRIBUTION.md`) on the trunks through a species' `bark` field, with tangents from the generator for the normal map; the roughness map is left out because Bevy reads metalness from the same texture's blue channel. Leaf cards come in two procedural styles by the species' `leaves` field, a cluster of veined broad leaves and a spray of needles, at 256 pixels.

**5e: impostors and alien species.** An octahedral impostor baker over the tree variants (a lattice of views into one atlas per species, blended by view vector in the shader) replacing the card LOD, and its wind kept in phase with the mesh LODs by sharing the global term of the bend. Two alien species as data: one tree whose generator parameters, palette and emissive make it read as alien on the same renderer, one grass with a different blade profile. Exit: the far LOD is an impostor with no visible switch at the band in a still; the alien biome (a new `.biome.ron` claiming a climate window) reads as alien in a still with no shader change.

**5f: rain, hydration and cover.** Decided in ADR 0006. Two halves, the second on the first.

- *Worldgen.* `WorldParams` gains the prevailing wind (seed-chosen direction with a slow large-scale turn), the rain march (tap count and spacing, land decay, orographic rate, the coarse continent's octaves) and the hydration and cover dials (hollow radius and gain, slope loss, cover wavelength and contrast, the hydration curve). `ProceduralWorld::rainfall(x, z)` marches the taps; `climate` takes its moisture from it (with a small share of the old noise); `Sample` gains `hydration` and `cover`. `apps/worldmap` writes `-moisture`, `-hydration` and `-cover` maps. Tests: determinism across threads for the new fields, the seed 42 snapshots regenerated, a rain-shadow test (over seed 42, the mean moisture on the windward side of the highest range within 50 km of the origin exceeds the lee side's), a cover-variance test inside one forest biome, and `tests/perf.rs` reporting the sample cost against the old figure, under twice it. Exit: the moisture map shows dry lee sides and wet coasts facing the wind; `just ci` green; the number in this section.

  Done 2026-09-12. What landed: `ProceduralWorld` gained a seed-chosen prevailing wind (`seed.rng("climate/wind")` through `libm::cos`/`libm::sin`, turned by a one-octave noise at `wind_turn_wavelength_m` 400 km up to `wind_turn` 0.35 of a tangent, renormalised by `sqrt` rather than rotated so no `sin` is called per sample) behind `wind_at(x, z)`, and `rainfall(x, z)` marching `rain_taps` 5 taps at `rain_tap_spacing_m` 3600 m, an 18 km reach. A tap evaluates a new private `coarse` continent: the real continent and range fractals truncated to `rain_coarse_octaves` 2 by `Fractal::truncated`, which keeps the full stack's normalisation on purpose so the coarse field agrees with the real one about where the land is (a renormalised truncation moved every coast and inverted the shadow). The parcel starts full, loses `rain_land_decay` 0.24 of what is left per tap over land, and loses `rain_orographic_rate` 0.9 of it per unit of rise, a unit being `rain_rise_scale_m` 200 m between taps; the rain-out at the sample is added to the result instead of subtracted, so a windward slope is wetter than the flat coast the same distance inland. `climate` now takes moisture from the march with `rain_noise_share` 0.25 of the old stretched noise left in, and `coastal_moisture`, `coast_dry_start` and `coast_dry_end` are gone, the march having subsumed them. `Sample` gained `hydration` (the moisture plus `hydration_hollow_gain` 0.25 times how far the point sits below the mean of four taps at `hydration_radius_m` 250 m, scaled by `hydration_relief_scale_m` 40 m, minus `hydration_slope_loss` 0.3 times the slope, clamped) and `cover` (`smoothstep(cover_dry` 0.3`, cover_wet` 0.7`, hydration)` times a two-octave patch noise at `cover_wavelength_m` 600 m stretched by `cover_contrast` 1.6). The hydration taps read a two-octave truncation of the winner's relief, because a hollow that holds water is a shape of the land and not of its grain. `apps/worldmap` writes `-moisture` (a sand-to-blue ramp), `-hydration` and `-cover` (grey) next to the biome and height maps, with the wind and the three land means in its log line; `docs/research/worldmap-seed42*.png` are regenerated. Sample cost on the plan's machine: 465 ns before, 852 ns after, 1.83 times, measured with `cargo test --release -p strata-worldgen --test perf -- --ignored --nocapture`. Seed 42 over 100 km: 51% land, wind (0.97, 0.24), land means moisture 0.54, hydration 0.52, cover 0.24, and the biomes still spread over coast 12.6%, plains 13.2%, tundra 10.5%, forest 9.4%, desert 3.0%, mountains 2.6%, savanna 2.4%, highlands 2.0%. The rain-shadow test over seed 42's highest point within 50 km of the origin reads 0.517 mean moisture 5 to 15 km upwind against 0.175 downwind; the cover-variance test finds a standard deviation of 0.35 on a 64 m grid inside a forest. One correction that cost a debugging round and is worth remembering: the coarse march's land mask must reach the rest of the tap continuously, so `coarse` skips only the range noise where the land mask is a hard zero and never the shore and elevation terms, or the step lands in moisture and a biome border becomes an edge.
- *Consumers.* `strata-flora`: a species' fit times `cover` raised to a per-kind power (trees 1.5, bushes 1.0, grass 0.4) and grass density times `hydration`; the species files get a `cover_floor` so a plains oak still stands alone in a dry patch. `strata-terrain`: `cover_of` multiplies the biome's canopy cover by the sample's `cover`, and the far tint leans towards a drier colour as `hydration` falls. `strata-foliage`: the grass record's `lush` byte comes from `hydration`. Exit: a still of a forest with clearings and a ragged edge, the approach flight showing the same clearings in the far canopy, and a still of a valley greener than its ridges.

  Done 2026-09-12. What landed: `strata-flora::cover_gain(kind, cover, cover_floor)` is the one place the new fields enter placement, and `fit_at` multiplies a species' biome fit by it. The per-kind powers of ADR 0006 are built from `sqrt` because this crate calls no platform transcendental per sample: a tree's `c^1.5` is exactly `c * sqrt(c)`, a bush's `c^1` is `c` itself, and grass' `c^0.4` is the three-term fit `0.98 * sqrt(c) + 0.22 * sqrt(sqrt(c)) - 0.2 * c^0.75`, which is monotone from zero to one and within 0.007 of the true power over the whole interval (the plain `0.6 * sqrt + 0.4 * sqrt(sqrt)` pair was 0.029 off at the dry end, and a test holds the fit to its tolerance). `FloraDef` gained `cover_floor` (serde default 0, validated `0..=1`, copied into `Species`), mixed in as `floor + (1 - floor) * curve` rather than taken as a maximum so the curve stays monotone and still reaches one at full cover; the shipped files set oak 0.1, scrub 0.3 and both grasses 0.5, so a plains oak stands alone in a bare patch while a grove is still the difference between cover 0.3 and 1.0. Grass density is `0.4 + 0.6 * hydration` applied to the hash threshold and not to the grid, so the candidate slots of a cell are the same wherever the ground is wet and only the share that survives moves; the blade record's `lush` byte now quantises `hydration` instead of `moisture`, so the shader's dry-to-lush tint pair follows the ground and not the region. `strata-terrain::cover_of` multiplies the biome pair's canopy alpha by the sample's `cover`, and `tint_of` leans the biome's own tint towards `tint * [1.15, 1.05, 0.7]` by `(1 - hydration) * 0.5`, which keeps every biome recognisable instead of mixing in a fixed sand. `strata-foliage` needed nothing. The counts, over the eight by eight cells (512 m) around the forest at `-2528,-37088` on seed 42, where the mean sample cover is 0.35: 2480 trees and 1.10 M blades before, 827 trees and 0.65 M blades after, the before measured by wrapping the world in a source that forces `cover` and `hydration` to one, which is exactly the old behaviour. In the window that spot logs `foliage cells=512 blades=4541969 trees=32287`, against `cells=512 blades=5012174 trees=51307` at `22048,30240`, where cover is 1.0: the same biome, half again the trees. Over 222 pure forest cells within 50 km of the origin the sample cover means 0.47, with 84 cells over 0.6 and 64 under 0.2, which is the grove-and-clearing spread the stills show: the ground still at the dry spot is open woodland with shadows on grass, the still at the dense cell is a closed grove, and the approach flight at 200 m/s shows a ragged forest edge with a dense band along one ridge thinning into scattered trees, with the clearings between the groves reading as bare ground in the far canopy colour. The hydration tint shows at a kilometre or two: the flight over the largest hydration spread found on a 300 m sweep (0.25, at `-1500,-12000`) has green hollows against pale tan rises in one frame. The local valley-to-ridge signal is honest but subtle at this lean, since the hydration field's own spread over a few hundred metres is about a quarter of its range; the tint difference across biomes and regions is the larger part of what the eye gets.

**Integration.** The debug HUD's foliage line, CLAUDE.md and README updated for the two crates and the flags that survive (`--no-foliage`, `--wind`), `just server-check` extended to build and test `strata-flora` on its own, ADR 0005 referenced from the RFC that records the delivery, the spike branch deleted. Exit: `just ci` green, the Phase 3 flight measured again with foliage on, and the standalone and connected Phase 4 measurements repeated with the numbers in this section.

### Phases 6 to 12: asset refactor and first world slice

Planned 2026-09-26 in [WORLD_SLICE_PLAN.md](WORLD_SLICE_PLAN.md), from the
[asset pipeline discovery](research/asset-pipeline-discovery.md). Every asset
moves to a `just content` step over ashlar with game-side generators, terrain
gains erosion and stylised procedural layers, trees, scatter, rocks and cliffs
are rebuilt, and the world gets its first settlement: a small metropolis on a
seeded site, reached by road. Each phase ends at measured performance gates
recorded in `docs/perf/ledger.md`. This supersedes the note above that rocks,
props and the first interior cell go to Phase 6.

**Done 2026-09-26 to 27.** Every phase's record, with its measurements, is in
WORLD_SLICE_PLAN.md:

- Phase 6: the content step and the bench harness.
- Phase 7: eroded terrain and stylised layers.
- Phase 8: ashlar's props, cut-outs and impostors.
- Phase 9: baked trees and far-tree batches.
- Phase 10: rocks, understory, groves and children.
- Phase 11: Port Serein, placed by the world function (ADR 0009).
- Phase 12: the route and acceptance.

The refactor is finished. The slice is walkable from the wilderness into the
city's lobby, alone or two to a server. The dense forest fixtures and the
flight still miss their frame-time gates, with owners named in its Phase 12
record.

## 6. Open questions

Vegetation follow-up, 2026-09-12: persistent client cells replace terrain-owned
foliage lifetimes, with bounded placement, incremental tree LOD installation,
retirement and a CPU placement cache. Grass gains small patches, tufts and
species blade profiles. Eight-view impostors now represent actual tree variants;
the full octahedral/elevation-view baker and alien species remain open from 5e.
Height-aware grass LOD, triangle ground attachment, previous-frame growth and
rebase wind motion vectors are corrected. See the amendment in ADR 0005 and
`docs/research/foliage-improvements.md` for budgets, tests and measurements.

- **Anti-aliasing and post-processing**: [ADR 0007](adr/0007-render-look-studies.md) adds opt-in camera looks, an F6 tuning panel, fixed mist fixtures and reproducible captures. Baseline remains ACES/MSAA 4; the occlusion and mist comparisons use TAA. The [study](research/render-look-study.md) records the visual direction and measured costs. A production default, hashed/dithered foliage alpha and global weather remain open.
- **Grass on the GPU**: ADR 0005 builds the per-cell grass records on the CPU and uploads a raw buffer. Generating them in a compute pass from a per-cell height texture (the Ghost of Tsushima shape) removes the CPU build and the upload; the step is contained in `strata-foliage` and is taken when the build shows in a profile.
- **Terrain material approach**: decided in Phase 3 for a texture array splat as a `MaterialExtension` over `StandardMaterial`, planar top-down mapping, with the vertex attributes listed in `strata-terrain/src/material.rs`. Triplanar mapping is still the upgrade if cliffs ever get steep enough to stretch.
- **Texture repetition**: solved for the slice by stochastic tiling (hashed offsets per vertex of a triangular lattice, three reads blended). What remains is a faint grain where the contrast renormalisation varies across a triangle; histogram-preserving blending over gaussianised textures would take that out if it ever shows.
- **big_space**: adopt the crate or keep our own origin rebasing. ADR 0002 starts with our own; revisit if the rebasing pass ever becomes the bottleneck or if we need nested grids.
- **Physics**: decided in Phase 4a for `avian3d` 0.7 in `f32` render space with the colliders rebased with everything else (ADR 0002). The dedicated server (4b) keeps one `RenderOrigin` that follows the first connected player, since with no `PrimaryObserver` the first observer stands in; a second player fifty kilometres away is stepped in `f32` at that distance, which holds to a few millimetres and gets worse linearly. Per-player physics frames or avian's `f64` feature would remove the limit; the cost of an `f64` build is unmeasured.
- **Reconciliation**: 4b corrects the predicted player when the server's position for an acked input is more than half a metre from the prediction for that input, by shifting the player and its recorded predictions by the error (ADR 0004); the server holds a player still through a stall so the ack stays exact, and the queue's lag (two ticks after a stall, on top of the two in flight) is the price. A stall while airborne still lets the arc finish on the server, which shows as one small correction. Replaying the unacked inputs from the acked state instead needs `CharacterController::step` runnable outside the schedule for several ticks (it is a plain method already; the `MoveAndSlide` system parameter is what ties it to a system), and the intents kept on the client until acked. Also open: characters collide with each other through the move and slide, so two players can block one another; walking through other players, or pushing them, is a physics-crate decision.
