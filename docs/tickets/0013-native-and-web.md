# 0013: Native and web

Status: designed 2026-10-15; steps 1 (the rename), 2 (the ports) and 3 (the web build) built.

## What the user said (2026-10-14, night)

> "prepare the game for pushing to a new github repo, where we should deploy
> it as a page (use browser storage for save states so people can play in
> their browser). make sure to sanitize/squash the push so that we don't push
> any license problematic or secrets."

> "refactor code, will make it cleaner anyhow. want to support native as
> well, so adapters. this is non commercial, so we are fine."

> "feel free to make the repo.. but we should actually find a better name and
> rename our crates etc."

Asked and answered the same night: the game and the repository are **Veyra**;
the engine crates `scrolls-*` become **`strata-*`** (the `veyra-*` content
crates keep their names); the data ids `scrolls:` become **`strata:`** too,
with the world version change and an upgrade of old save files that brings.

## The design

One codebase, two targets. Native stays exactly as it is; the web build is
`wasm32-unknown-unknown` on WebGPU. What differs between them sits behind a
small port with a native and a web adapter, chosen at compile time, so no
system knows which it runs on.

1. **Files.** Every read of game data at run time (the registries, the baked
   layouts in `generated/`, the flora library, the icon catalogue, the combat
   tables) goes through slotted's `AssetSource` port (`list`, `read`), which
   the registry already loads through. One process-wide source is installed at
   startup: natively a directory source over `assets/` and `generated/`, on
   the web an in-memory source filled from a data bundle fetched before the
   app starts. Large binary assets (textures, meshes, sounds) keep going
   through Bevy's asset server, which fetches them over HTTP on the web.
2. **Saves.** The household file and the settings go through a `Store` port
   (`read(key)`, `write(key, text)`): natively the files they are today, on
   the web `localStorage` under the same keys. The file's versions and
   upgrades are unchanged; an old `scrolls:` file is upgraded on read.
3. **Work off the frame.** The world generator's few `std::thread::scope`
   parallel maps go through one helper in `strata-core` that runs them on
   threads natively and in turn on the web. Bevy's task pools already run on
   the main thread in a browser; a heavy task there costs a hitch, not a
   crash.
4. **Time.** `std::time::Instant` panics in a browser; one re-exported
   `Instant` (the `web-time` crate's on the web, std's natively) replaces it.
5. **The app.** Natively the game parses its command line as today. On the
   web the same `Cli` is parsed from the page's query string (`?at=camp`),
   with defaults suited to a browser (a smaller world bake round the spawn
   before start, no multiplayer). The network crates stay native only.
6. **The build.** `just web` builds the wasm, binds it, packs the data bundle
   and copies the assets the game loads into `dist/`. A GitHub Pages workflow
   bakes `generated/` with `just content`, runs `just web` and publishes
   `dist/`.

## The plan

1. The rename (scripted, one commit), with the world version pin and the save
   upgrade.
2. The ports and their native adapters: nothing changes natively and every
   test passes; the workspace checks for `wasm32-unknown-unknown`.
3. The web adapters, the web entry and `just web`; the game runs in Chrome
   under a virtual display and a still is taken.
4. The public repository: a sanitized single commit, the Pages workflow, the
   push.

## As built

### Step 1: the rename

- A one-off script renamed every `scrolls-*` crate and directory to
  `strata-*`, the facade `scrolls` to `strata`, `scrolls_*` paths, the data
  ids `scrolls:` to `strata:` and the string and theme keys `scrolls.` to
  `strata.`; the English verb, "Elder Scrolls", the checkout's name and
  the settings folder `sci-fi-scrolls` stay.
- The world did not move. Some seeds are derived from id strings (each
  biome's relief, each province's beds, each species' seed, a faction's
  name salt) and the world version hashes ids and definitions, so a plain
  rename moved every world. `strata_core::stable_label` gives an id its
  old spelling and `Seed::derive`, the factions' salt, the settlement's
  ground hash and `world_version`'s hasher read through it: every world
  pin, canary and snapshot number holds (the drama snapshot changed only
  its column padding). The hash salts `b"scrolls world"` and
  `b"scrolls combat"` keep the old word for the same reason.
- Saves: a household file under the old ids is read with them renamed and
  kept beside as `.ids.bak` (`OLD_DATA_NAMESPACE`, with a test). The
  settings file holds no ids.
- `just content` rebaked `generated/`: the tree library and Port Serein's
  layout name species, so their versions in the handshake changed.
- Left rough: the checkout's directory and the settings folder keep the old
  name; `sci-fi-scrolls` appears in docs as the local path.

### Step 2: the ports

- `strata_core::platform` (no Bevy): `par::{map, map_index, threads}` is the
  one parallel map (scoped threads capped at 16 natively, in turn on wasm32;
  callers keep their own one-pool lock) and `time::{Instant, SystemTime,
  UNIX_EPOCH}` are std's natively and `web-time`'s on wasm32. Every
  `std::time::Instant` in the crates the game reaches uses it.
- Files: `strata_registry::files` holds the installed `AssetSource`
  (`install`, `current`, the cloneable `Files` with `under` and a cheap
  `exists` answered from a listing). The native adapter is `MountedDirs`
  (logical roots `assets/` and `generated/`); the game installs it before
  anything reads. The combat tables, skill graph, tree library, site,
  crossing and roadside kits, versions, water prep, art indexes and the
  grounding study read through it; path-taking wrappers stay for the server.
- Saves: `strata_sim::store::Store` (`read(key) -> Result<Option<String>>`,
  `write`), native adapter `FileStore` (key is the path, parent made, the
  `.bak` copies kept); household and settings use it. Developer outputs
  (trace, script reports, bench JSON, render-look export) are native only.
- Features: `net` (default) on apps/game carries `strata/net-replicon`;
  `web` is empty. `cargo check -p game --target wasm32-unknown-unknown
  --no-default-features --features web` passes, no renet or replicon in
  the graph, with no `.cargo/config.toml` change needed.
- Per crate for wasm32: core (platform), worldgen (par), sim (store, files
  reads), registry (files), flora, physics, atmosphere, foliage, terrain,
  client (Instant, SystemTime, source-taking art indexes), game (features).
- Left rough: native file errors name the logical path, not the real one;
  `--render-look-file` and `grounding.rs`'s output bake still use std::fs
  natively; nothing runs on wasm yet (step 3).

### Step 3: the web build

- `just web` builds `dist/`: the release wasm (`--no-default-features --features
  web`), `wasm-bindgen`, `wasm-opt -O2`, `data.bundle.gz` (`xtask bundle`: 346 files,
  185 MB raw, 31 MB by a `gzip -9` subprocess), `index.html`, `assets/`, `generated/`
  (without `capacity`; `materials/capacity` stays, it holds leaf textures).
- Adapters: `strata_registry::bundle`, `strata_sim::store::LocalStorage` (keys
  `veyra/household.ron`, `veyra/settings.ron`, the `.bak` copies); `--data-bundle <raw>`
  runs the bundle natively with no missing-file warning a normal run lacks.
- Entry (`apps/game/src/web.rs`, `query.rs`): `index.html` unzips the bundle into
  `window.VEYRA_BUNDLE`, the wasm reads and clears it; `?at=city&fresh` becomes the `Cli`;
  the entry inserts `PointerLockOnClick` and `SettingsFile` before the client plugins,
  so no platform cfg sits in a system. Start-up errors and early panics reach
  `console.error` and replace the loading line (`fail`).
- Bevy on the web: `default` less `multi_threaded`, `webgl2`, gilrs, X11, Wayland,
  `bevy_clipboard`, `sysinfo_plugin`, plus `webgpu`, `web`. `POLYGON_MODE_LINE` and
  `IMMEDIATES` are native only; nothing in the game uses immediates.
- The browser needed: no PCSS (Tint refused it; `pcss` feature, `SOFT_SHADOWS` hides the
  Render row); `tree_impostor.wgsl` with gradients before its `discard` (the one edit under
  `assets/`, in no version); `AssetMetaCheck::Never` (404s); a click before the pointer lock.
- Measured (software Vulkan under Xvfb, no benchmark): rivers bake 71 s on one wasm thread
  (7.5 s native on 16), first frame 74 s, 25 fps at 1080p as native here; wasm 192 MB, 89 MB
  after wasm-opt (9 min); `dist/` 684 MB. Stills: `docs/design/web/`.
- Left rough: stills not like for like (camera, time; web facades read bluer, PCSS off is
  a candidate); audio and the impostor shader's native GPU cost unmeasured; foliage is slow.
