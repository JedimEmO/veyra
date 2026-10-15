# RFC phase1: the engine foundation

Status: landed, 2026-09-11 to 2026-09-12. Commits `9666f6e` (the slice), `7005621` (Phase 4 plan), `e65775f` (Phase 4). Covers plan Phases 0 to 4. Companion documents: `docs/PLAN.md`, `docs/adr/0001` to `0004`.

## 1. Why this document exists

The first two days built a lot of machinery quickly, most of it by agents working from the plan and the ADRs. This RFC is the record of what was decided and why, in one place, so that a later change to any of it starts from the reasoning and not from a guess. Every choice below names where it lives in the tree and what it would take to change it.

## 2. What was delivered

A game engine on Bevy 0.19.1 for a sci-fi MMO-like RPG, as a workspace of layered crates, and a game binary that shows:

- A seeded procedural world, seamless over more than 100 km, with nine biomes, continents, coasts, mountains to 3000 m, deserts and polar snow, drawn to the horizon at 200 fps and more at 1440p on the development machine.
- A character walking on that world through a fixed-step simulation and a physics ground, with a third-person camera.
- A dedicated server with no renderer that owns the simulation and serves it to several clients, each predicting its own character.
- The first UI screens on the slotted crates: a pause menu with persisted settings, a HUD layer, a hotbar bound to an inventory.

Everything is exercised by `just ci` without a GPU: 280 and more tests, clippy with warnings fatal, and a check that the headless and server graphs contain no render crate.

## 3. The choices, and the intention behind each

### 3.1 Layered crates with a Bevy-free bottom (ADR 0001)

`strata-core`, `strata-registry` and `strata-worldgen` have no Bevy dependency. `strata-sim`, `strata-stream`, `strata-physics` and `strata-net` see `bevy_ecs`, `bevy_app` and `bevy_math` and nothing that renders. `strata-terrain`, `strata-atmosphere` and `strata-client` are the render layer. `strata` is the facade with two plugin groups, `headless()` and `client()`, where the client group is the headless group plus rendering.

**Intention.** An MMO server runs the world with no window. Making that a plugin group from day one, and asserting it in CI with `just server-check`, means the server is never a refactor. The pattern is lifted from bevy-slotted, which has the same shape and the same check.

**To change it.** The rule is enforced only by the justfile allowlist and by dependency declarations; a crate that needs to move layers moves its `Cargo.toml` and the allowlist and the check tells you if the graph leaked.

### 3.2 The world is a pure function of the seed

`strata-worldgen::WorldSource` is `sample(x, z) -> Sample` over a seed and frozen registries, allocation-free per sample, `Send + Sync`. Stages: domain-warped continents, a climate field (temperature from latitude, altitude and noise; moisture from noise and a coastal proxy), biome classification by fit against each biome's climate ranges with a soft blend between the two best, relief per biome kind (ridged for mountains, billow for dunes, gentle fbm for plains), and a surface material with a slope override.

**Intention.** Server, client, tests and the map tool must all get the same bits from the same coordinates, or the client's ground and the server's ground disagree. Determinism is tested across threads with proptest and pinned with insta snapshots. Biome and material definitions are RON data loaded and frozen through `slotted-registry`, so tuning is data and, later, moddable.

**Alternatives not taken.** Voxels (ADR 0003): an order of magnitude more memory and meshing time, and biome-driven relief is harder to author. Caves and interiors are to be separate cells attached to the heightfield, the way Elder Scrolls does it; a hole mask per region and an edit layer on `WorldSource` are the reserved hooks.

**Where.** `crates/strata-worldgen`, `crates/strata-registry`, `assets/biomes/*.biome.ron`, `assets/terrain/materials.ron`, `apps/worldmap` (renders a seed to PNG; `docs/research/worldmap-seed42.png`).

### 3.3 `f64` in the world, one movable render origin (ADR 0002)

`WorldPos(DVec3)` is the only position type below the render layer. `RenderOrigin` is a resource; a rendered entity's `Transform` is derived every frame as `(world - origin) as f32`, and the origin jumps to the primary observer whenever it is more than 2 km away, rewriting every transform and every physics body in one pass. A debug assertion catches any system that writes `Transform` directly on a world-positioned entity.

**Intention.** Tens of kilometres now, planetary later, with no jitter. The alternative was `big_space`; it was not taken because it replaces transform propagation for every render-aware crate and its nested grid is more than a heightfield world needs. It stays an open question if the rebase pass or nested frames ever matter.

**Consequence that surfaced in Phase 4.** The server has no camera but has an origin, because physics runs in `f32` relative to it. With several players it follows the first one; a player 50 km from it is stepped at a few millimetres of precision. Per-player physics frames or avian's `f64` build would lift that. Open in the plan.

### 3.4 Chunked quadtree heightfield with skirts and morphing (ADR 0003)

Regions are `RegionCoord { x, z, lod }` with a 64 m base and a 65x65 vertex grid at every level, so lod 0 is one vertex per metre and lod 8 spans 16 km per node. `strata-stream` selects a disjoint set of regions around each observer by a split factor, with hysteresis, a per-frame budget, and a guarantee that a parent stays until its children are ready, so there is never a hole. `strata-terrain` meshes each region on the compute pool, with normals from the samples so they are continuous across LOD, skirts to hide cracks, vertex morphing towards the parent to hide popping, and a correct bounding box for culling. Colliders (Phase 4) sample the same grid and match the mesh bit for bit.

**Intention.** The region grid is one unit for everything: streaming, collision, persisted edits later, and the server's interest management (3.7). Clipmaps were rejected because they are camera-centred and continuous and fit none of those.

**Where.** `crates/strata-stream` (pure selection, headless tests), `crates/strata-terrain/src/mesh.rs` (pure builder, tested), `crates/strata-physics/src/terrain.rs`.

### 3.5 Terrain look: texture arrays, stochastic tiling, atmosphere

A `MaterialExtension` over `StandardMaterial` so lighting, shadows and fog come free. Six CC0 Poly Haven ground sets at 2k stacked into diffuse, normal and roughness texture arrays in registry order, blended two layers per vertex plus a slope-driven rock layer, with a biome tint at distance. Repetition is broken by stochastic tiling over a triangular lattice with hashed offsets and three blended reads. Bevy's `Atmosphere` with a sun bound to sim time of day, cascaded shadows, and a depth-attenuated water plane at sea level. The modelled planet is itself a world-positioned entity so origin rebasing moves it.

**Intention.** The slice had to read as a world, not a heightmap. Each of these was chosen after reading screenshots, and the review found and fixed the mismatch between hardcoded layer order and registry order, the texture grid at mid distance, the glass-like sea and the horizon bar.

**Where.** `crates/strata-terrain/src/material.rs`, `assets/shaders/terrain*.wgsl`, `crates/strata-atmosphere`, `assets/textures/ATTRIBUTION.md`.

### 3.6 A character through the simulation

`Player`, `PlayerIntent` (move, yaw, jump, sprint) and `LocalPlayer` live in `strata-sim`; intents are consumed into a command once per 20 Hz tick, and `strata-physics` steps a kinematic capsule with avian3d's move-and-slide, gravity, a 50 degree slope limit, step-up, ground snap and swimming below sea level. The player is the streaming observer and the render origin's anchor. The client interpolates between the last two ticks for the camera and the visual.

**Intention.** The same code moves the player whether the process is standalone, the server, or a predicting client. That is what made 3.7 small. The flycam is kept behind `--flycam` for flight verification.

### 3.7 Networking (ADR 0004)

`bevy_replicon` 0.44 over renet, behind the `strata-net` port, in the `strata-net-replicon` adapter. The server owns the sim and replicates positions and players; a client sends only sequence-numbered intents; the client predicts its own player with the same physics and corrects against exact acks; interest management is the region quadtree. See section 5 for the model in full.

**Intention.** Take the byte framing, reliability, change detection and per-client visibility from a maintained crate and spend the effort on the decisions that are the game's: authority, prediction, interest.

### 3.8 The first slotted screens

`slotted` wired into `strata-client` behind a `ui` feature with the glass theme; a pause menu from `slotted-menu` templates whose settings persist and apply (field of view to the projection, sensitivity to the camera), the debug statistics on a HUD layer, a reticle and an 8-slot hotbar bound to an empty `Inventory`, all driven headless in tests by `slotted-test`.

**Intention.** The inventory model is in the tree from the first screen, and every screen from now on is testable without a GPU. Two slotted gaps found while doing it are recorded in the plan for the slotted repo: the harness never calls `App::finish`, and the theme plugin inserts a default theme handle.

### 3.9 The simulation does not pause under a menu (ADR 0004)

A menu takes the cursor and the steering away and clears the local intent; the clock, streamer, physics and replication keep running. This is an MMO. A single-player pause, if ever wanted, is a standalone-only freeze of the fixed clock.

### 3.10 bevy-slotted as a git dependency with a local patch

Pinned to rev `3cc4f01` in `Cargo.toml`, patched to the sibling checkout in `.cargo/config.toml`. A clean clone builds from GitHub; local work sees the local crate. The cost: `Cargo.lock` records path sources on every local build, so CI runs without `--locked`. Bevy versions must match across both repositories.

## 4. What was left out on purpose

- **Assets** (Phase 5): scatter of rocks and trees, `slotted-packs` as the asset source, props, the first interior cell. Deferred so Phase 4 stayed about the loop.
- **Reconciliation**: correction by shift, not rewind-and-replay. Needs the character step callable outside the schedule.
- **Security**: netcode runs unsecure, the client picks its own id, no accounts, seed not replicated.
- **Player against player collision policy**: kinematic capsules block each other; spawns are spread 2 m apart.
- **Reliable intent channel**: a lost packet stalls up to the resend interval; an unreliable channel with redundancy is the MMO-grade choice.
- **Snapshot buffer for remote players**: they are drawn from the latest replicated tick; a late packet is a small hitch.
- **Pipeline warm-up**: the first terrain frame costs about 250 ms of shader compile.
- **Shoreline**: worldgen's shore step meshes as a small staircase; a faint rim at the water's edge remains.

## 5. The networking model

The simplest description: **the server runs the game, clients send what they want to do, and the client cheats a little for its own body so it feels immediate.**

**Roles.** A process is `Standalone` (the slice, one sim, no wire), `Server` (headless facade, listens on UDP) or `Client` (renders, connects). The sim, the streamer and the physics are the same code in all three.

**What crosses the wire.**
- Server to client: `WorldPosition` and identity of every player the client is allowed to see, a singleton carrying the sim clock, and a `Welcome` event naming the client's own player.
- Client to server: `PlayerIntent` with a sequence number, one per tick. Nothing else. The server validates it (normalised move vector, finite yaw) and never trusts a position from a client.

**Authority.** The server applies at most one intent per player per tick, in order, and holds the player still on ticks where none arrived. Its physics is the truth; the position it sends for a player is stamped with the sequence of the intent that produced it.

**Prediction.** The client's own player runs the same `CharacterController` as the server, so a key press moves it on the next tick with no round trip. The client remembers the position it predicted after each sequence. When the server's position for sequence N arrives, the client compares it with its own prediction for N, not with where the player is now. If they differ by more than half a metre the two simulations genuinely diverged (a dropped input, a collider one side had before the other), and the client shifts its player and its whole recorded history by that difference. A shift rather than a set, because setting would rewind the ticks after N and every later ack would disagree by exactly that, cascading into a correction every few ticks. With this rule a two-tick stall produces no correction and a six-tick stall exactly one.

**Interpolation.** Remote players carry no physics on the client; their positions arrive at the tick rate and are drawn interpolated between the last two ticks, the same way the local player is drawn between its own ticks.

**Interest management.** Every connected player is a streaming observer on the server, so the server streams and unloads regions and colliders around each of them. For each client, the server walks the same quadtree selection the client uses and marks an entity visible when it falls in a region selected at LOD 2 or finer, about 1.3 km. Replicon's per-client visibility mask then filters what is sent; an entity leaving that radius is despawned on the client and respawned if it returns.

**Timing.** The sim runs at 20 Hz on both ends. Intents ride a reliable ordered channel; replication is sent at the fixed rate after the tick. The client's HUD shows role, round trip time, players in sight and the number of corrections taken, which is how the model is watched.

**What is deliberately not there yet** is listed in section 4: rewind-and-replay, authentication, an unreliable intent channel, and a snapshot buffer.

## 6. Measurements at landing

All release builds, 2560x1374 client area, vsync off, RTX 4090.

| Scenario | Result |
|---|---|
| 60 s flight at 200 m/s into a mountain range | 200 to 241 fps, about 850 regions loaded, worst frame after warm-up under 40 ms |
| Character walking, standalone, HUD on | 183 to 237 fps |
| Server tick with two connected clients | 0.2 to 0.4 ms per tick |
| Two clients on a localhost server | round trip 13 to 19 ms, zero corrections while walking |
| Sampler, 67x67 grid, one core | under 2 ms |

## 7. How it was built

The plan and ADRs 0001 to 0003 were written first, by hand. The code was produced by two orchestrated runs of at most three parallel agents, one per plan phase group, each agent working from the plan and required to pass `just ci` on what it touched. Each run ended with two independent reviewers, one on architecture against the plan and one running the binaries and reading screenshots, followed by a fixer; the review findings and what was fixed are in the plan's phase sections. Everything a reviewer flagged and the fixer left is in section 4.
