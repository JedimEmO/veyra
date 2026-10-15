# World streaming: what a moving frame pays, and the plan (2026-10-04)

The question after lighting plan phase 6 (`profile-2026-10-03.md`, "What
phase 6 did"): the settled views are under the 8.3 ms gate on the render
thread, and the three moving fixtures are not. This document is what a
frame pays while the observer moves, on the main thread and the render
thread, where the hitches come from, and a phased plan to remove them. It
is built from two investigations at commit `2515e08` (Phase 6, the render
thread): a traced profile of the moving fixtures joined to a per-frame
record of the streamers, and a read of the streaming design end to end
against the code and today's bench logs. No code in the repository was
changed for either.

The headline: one cost dominates. Every terrain region goes to the GPU as
a 575 KiB Bevy `Mesh`, and Bevy interleaves it on the render thread one
attribute of one vertex at a time, at about 3.9 ms a region. The terrain
budget lets two through a frame, so every frame that uploads terrain costs
7.7 to 7.9 ms more on the render thread, and the same 7.7 ms task is often
run on the main thread too, inside a main-world system that is waiting on
its own parallel work. That one cost is the flight's render p50, the walk
and route-walk render p95, most of every fixture's worst main frames and,
with the water material, its worst render frames. Colliders, the
streamers' per-frame walks and the foliage collector's overruns come after
it, a millisecond or two each.

## Method

- **The traced profile.** Bevy's systems traced (`--features trace`, the
  earlier profile's tracer and trace summary) while the observer moved:
  `flight`, `walk` and `route-walk` for 60 s each, plus the settled
  `woodland` as the static control. Every run went through
  `tools/bench.py --xvfb` at 2560x1440 with `--stats`, one game at a time,
  the GPU held at full clock (`GpuPowerMizerMode=1`) and put back to mode
  2 afterwards. A scratch copy of the tree recorded one CSV row per frame:
  regions loaded and unloaded, terrain meshes inserted (vertices, bytes,
  build time), colliders and trunks, foliage placements, spawns and
  uploads, probe bakes, `Mesh` and `Image` asset events, and the depth of
  every task queue, joined to the trace by frame number. Each moving
  fixture was also run untraced so the hitch timings are clean; the
  tables that split frames by upload use the untraced runs. Each span's
  world (main, render, extract) comes from a dump of each world's
  registered schedules, taken at `Plugin::finish`.
- **The design read.** The stream selector, terrain meshing, colliders,
  the foliage cell streamer, probes, the fog clipmap, settlements and the
  rebase, read in the code together with the Bevy 0.19.1 and avian 0.7
  internals they lean on, against today's bench logs
  (`scratch/bench/{flight,walk,route-walk}.log`, 04:47 to 05:06) and a
  Python port of the selector that counts region churn per speed.
- **Checked for this document.** Where the two disagreed, or a fix depends
  on a detail, the code was read again: `collect_meshes` and
  `dispatch_meshing` (`crates/strata-terrain/src/lib.rs`),
  `RegionMesh::into_mesh` and the attribute formats (`mesh.rs`,
  `material.rs`), `RegionCoord`'s derived `Ord`, `update_water`, the
  collider streamer, `spawn_tree_level`, Bevy's `SlabAllocatorSettings`
  and wgpu limit selection, and `bevy_tasks`' scope executor.
- **Conditions.** Under Xvfb the frame is paced at about 50 ms, so the
  game runs at 16 to 20 fps. The streaming budgets are per frame, so their
  throughput per second scales with the frame rate; on a real display at
  60 to 120 fps the same code uploads three to six times as many regions a
  second, at the same cost per upload frame. The tracer costs about 1.7 ms
  of render p50 (13.55 traced against 11.88 untraced on the flight), so
  the per-system tables rank costs and do not add up to the frame. Every
  fixture runs with `--fog 0`, so nothing here measures the fog clipmap.

## The bound today

The ledger's last section (`Phase 6 render thread`), moving fixtures,
against the gates: p95 at most 8.3 ms on each thread's work time, and no
frame over 33 ms in a moving fixture. Under Xvfb the wall-clock columns are
the paced 50 ms frame and say nothing; the work columns are the bound.

| Fixture | Main p95 ms | Render p50 ms | Render p95 ms | GPU ms | Entities | Warm-up | Gate |
| --- | --- | --- | --- | --- | --- | --- | --- |
| flight | 13.63 | 11.87 | 12.95 | 3.75 | 4 744 | 10 s fixed | work 13.63 > 8.3 |
| walk | 8.08 | 5.94 | 13.50 | 2.95 | 26 683 | 10 s fixed | work 13.50 > 8.3 |
| route-walk | 8.10 | 6.22 | 13.74 | 3.50 | 24 248 | 10 s fixed | work 13.74 > 8.3 |
| woodland (settled, for comparison) | 5.53 | 5.81 | 6.69 | 4.42 | 20 902 | 38 s settled | ok |

Today's untraced runs, for the work-time worst frames the ledger does not
carry (`WorkClock` samples from `--bench-out`):

| Run | Main p50 / p95 / max ms | Render p50 / p95 / max ms | Bench main p95 / render p95 |
| --- | --- | --- | --- |
| flight (182 m/s) | 7.37 / 13.50 / 27.1 | 11.88 / 12.97 / 30.8 | 13.50 / 12.95 |
| walk (0.4 m/s: the walker stalls) | 6.64 / 9.75 / 31.0 | 6.19 / 14.61 / 39.9 | 9.87 / 14.75 |
| route-walk (8.1 m/s) | 6.80 / 9.93 / 18.9 | 6.86 / 14.54 / 31.4 | 10.59 / 14.61 |
| route-walk after its first terrain fill (from 36.8 s) | 6.25 / 8.02 / 15.1 | 6.29 / 13.00 / 15.1 | |

Two of the three rows do not measure what they are named for. The walk's
character stops at (-494, -328) and moves 26 to 28 m in the measured
minute, and both walks' p95 is mostly the first terrain fill (about 680 to
800 regions at two a frame, 31 to 37 s), which outlasts the fixed 10 s
warm-up. After the fill the walk is at the woodland's level (render p95
6.76, main p95 7.20) and route-walk shows what walking really costs:
render p95 13.0 with only 3.15 terrain meshes a second. All three still
miss the gate, and for one reason, below.

## What the profile measured

### Thread work by whether terrain was uploaded

Untraced. Render work is for the render frame of main frame N-1, split by
how many terrain meshes N-1 inserted; main work is for main frame N, which
runs at the same time as that render frame.

| Run | Terrain meshes in the frame | Frames | Render work mean / p95 ms | Main work mean / p95 ms |
| --- | --- | --- | --- | --- |
| walk | 0 | 854 | 6.03 / 6.80 | 6.39 / 7.20 |
| walk | 2 | 334 | 14.20 / 15.03 | 9.53 / 15.24 |
| route-walk | 0 | 643 | 6.50 / 7.67 | 6.23 / 7.22 |
| route-walk | 1 | 24 | 7.26 / 13.85 | 7.45 / 8.16 |
| route-walk | 2 | 470 | 13.67 / 14.88 | 8.67 / 14.23 |
| flight | 2 (every frame) | 1 072 | 12.00 / 12.97 | 8.09 / 13.50 |

### Throughput per second

Untraced, last 60 s; the traced runs agree within about 10 %.

| | flight | walk | route-walk |
| --- | --- | --- | --- |
| fps under Xvfb | 17.9 | 19.8 | 19.0 |
| Regions loaded / unloaded per s (of which LOD 0) | 86.6 (39.3) / 89.9 (42.3) | 0.0 / 0.1 | 3.0 (1.1) / 3.5 (1.6) |
| Resident regions mean; waiting for a mesh mean / max | 1 087; 431 / 1 079 | 964; 94 / 665 (first fill, done at 31.5 s) | 1 075; 150 / 788 (first fill, done at 36.8 s) |
| Terrain meshes uploaded per s | 35.7 | 11.1 | 16.0 (3.15 after the fill) |
| Terrain upload MB/s | 21.0 | 6.5 | 9.4 |
| Meshing tasks finished per s; mean task ms | 86.6; 11.45 | none in the window (7.40 ms during the fill) | 3.0; 9.06 |
| Finished meshes dropped before upload per s | 61.1 | 0 | 0.07 |
| Main-thread `into_mesh` + `Assets::add` per mesh | 0.035 ms | 0.033 ms | 0.034 ms |
| Heightfield colliders built per s; task ms; main ms each | 39.2; 20.6; about 0.25 | 0 | 1.07; 16.9; about 0.25 |
| Trunk colliders spawned per s; main ms each | 215; about 0.04 | 0 | 14.1; about 0.04 |
| Collider entities added / removed per s | 254 / 296 | 0 / 0.7 | 15 / 26.5 |
| Foliage cells placed per s; place task ms | 60.9; 22.3 | 32.6; 13.6 | 38.9; 15.9 |
| Foliage entities spawned / despawned per s | 466 / 485 | 95 / 8 | 253 / 136 |
| Foliage collect ms mean / p95 / max; apply ms mean / max | 0.45 / 0.71 / 6.26; 0.17 / 3.09 | 0.45 / 0.63 / 6.47; 0.06 / 0.64 | 0.65 / 1.00 / 6.61; 0.10 / 0.52 |
| Probe bakes per s; bake ms | 0 (above the reach) | 0.13; 9.2 | 1.02; 9.5 |
| `Mesh` assets added / removed per s | 35.7 / 28.8 | 11.1 / 0.1 | 16.0 / 3.4 |

A region mesh is 4 485 vertices (the 65 by 65 grid and its skirts) at 108
bytes a vertex and 104 KiB of `u32` indices: 575 KiB.

### The async side

Bevy's async compute pool has its default four threads; nothing in the
game sets `TaskPoolOptions`.

| Queue | flight mean / p95 / max | walk mean / p95 / max | route-walk mean / p95 / max | Cap |
| --- | --- | --- | --- | --- |
| Terrain tasks held (`TerrainStats.in_flight`) | 454 / 1 058 / 1 107 | 101 / 565 / 679 | 158 / 690 / 799 | none: a task per `RegionLoad` |
| ...of which finished and waiting for the upload budget | 450 / 1 054 / 1 104 | 101 / 567 / 681 | 159 / 692 / 801 | |
| Frames that hit the terrain budget | 100 % | 28 % | 41 % | 1 MiB, checked before each insert, so 2 x 575 KiB |
| Collider tasks held; finished and waiting | 3.7 / 10 / 17; 2.3 / 6 / 9 | 0 | 0.1 / 0 / 8 | 16 heightfields and 256 trunks a frame |
| Foliage jobs in flight + ready | 4.6 / 7 / 8 | 1.7 / 4 / 4 | 2.1 / 4 / 7 | 8 in flight, 4 dispatched a frame |
| Async pool demand, thread-seconds per second | terrain 0.99 + colliders 0.81 + foliage 1.36 = 3.16 of 4, 0.70 of it meshes later dropped | foliage 0.44 | 0.68 in all | |

No frame waits on the pool: every collector polls only tasks whose
`is_finished()` is already true. The frame pays for the bursts that a
budget lets through instead.

### The largest systems on the flight

Traced, CPU ms per frame summed over threads, with the settled woodland
beside it. The walk and route-walk tables are in the scratch analysis
(Reproducing); their streaming rows are the same systems at smaller means.

| # | Render world | mean | p95 | max | woodland mean |
| --- | --- | --- | --- | --- | --- |
| 1 | `allocate_and_free_meshes` (in all 973 frames) | 7.747 | 7.848 | 19.11 | under 0.07 |
| 2 | `prepare_clusters_for_gpu_clustering` | 0.638 | 0.746 | 1.44 | 0.616 |
| 3 | `submit_pending_command_buffers` | 0.636 | 0.857 | 8.43 | 0.404 |
| 4 | `prepare_preprocess_bind_groups` | 0.297 | 0.409 | 0.80 | 0.503 |
| 5 | `prepare_erased_assets<WaterMaterial>` | 0.171 | 0.220 | 10.85 | 0.077 |

Walk and route-walk: `allocate_and_free_meshes` 2.320 and 3.431 mean,
7.80 and 7.84 p95, 18.22 and 17.42 max; the water material 0.117 and
0.128 mean, 10.37 and 9.30 max.

| # | Main world | mean | p95 | max | woodland mean |
| --- | --- | --- | --- | --- | --- |
| 1 | `update_mesh_previous_global_transforms` (the 8 ms spikes are the stolen render task) | 1.190 | 7.938 | 8.26 | 0.132 |
| 2 | `terrain::collect_colliders` | 0.671 | 2.486 | 6.87 | under 0.07 |
| 3 | `terrain::collect_colliders` (commands) | 0.482 | 1.529 | 3.00 | under 0.07 |
| 4 | foliage `streaming::collect` | 0.446 | 0.737 | 4.88 | 0.414 |
| 5 | foliage `streaming::collect` (commands) | 0.382 | 1.011 | 2.43 | under 0.07 |
| 6 | `terrain::dispatch_colliders` (commands: despawns) | 0.327 | 2.011 | 5.88 | under 0.07 |
| 7 | foliage `streaming::dispatch` | 0.287 | 0.365 | 0.88 | 0.419 |
| 8 | `strata_stream::diff_regions` | 0.273 | 0.362 | 1.34 | 0.162 |
| 9 | `strata_stream::select_regions` | 0.211 | 0.251 | 0.72 | 0.169 |
| 10 | `strata_terrain::collect_meshes` | 0.166 | 0.305 | 0.99 | under 0.07 |

`update_mesh_previous_global_transforms` is 0.479 mean (13.19 max) on the
walk and 0.606 mean (7.831 p95, 16.78 max) on route-walk.

### The hitches

Flight, the eight worst main frames (traced; the streamer columns count
regions loaded and unloaded, terrain meshes inserted, heightfields, trunks
and collider entities removed that frame):

| Frame | ms | Longest systems (ms) | Streamer that frame |
| --- | --- | --- | --- |
| 1060 | 23.8 | `update_mesh_previous_global_transforms` 7.98, `collect_colliders` 6.87, its commands 1.51 | ld 4, ul 4, terrain 2, heightfields 6, trunks 64, removed 18 |
| 1071 | 23.4 | stolen upload 7.96, `collect_colliders` 6.03, commands 1.38 | ld 8, ul 5, terrain 2, heightfields 5, trunks 61, removed 37 |
| 1085 | 23.2 | stolen upload 7.91, `collect_colliders` 5.39, commands 2.15 | ld 8, ul 1, terrain 2, heightfields 9, trunks 50 |
| 532 | 22.4 | stolen upload 7.89, `collect_colliders` 2.29, commands 1.44 | ld 8, ul 4, terrain 2, heightfields 6, trunks 32 |
| 267 | 19.7 | stolen upload 8.04, commands 2.82 | ld 8, terrain 2, heightfields 9, trunks 106 |
| 1006 | 19.6 | `collect_colliders` 6.27, commands 2.44 | ld 4, ul 9, terrain 2, heightfields 8, trunks 67 |
| 467 | 18.7 | `dispatch_colliders` (commands) 5.88, `collect_colliders` 2.73 | ld 4, ul 32, terrain 2, removed 299 |
| 324 | 18.6 | stolen upload 7.91, collider commands 1.76, foliage commands 1.37 | ld 8, ul 8, terrain 2, trunks 67, foliage spawns 180 |

17 of the flight's 20 worst main frames and all 20 of the walk's (17.4 to
24.1 ms) are headed by the stolen upload at 7.8 to 13.2 ms.

The worst render frames, all three fixtures:

| Run | Main frame | ms | Longest systems (ms) |
| --- | --- | --- | --- |
| flight | 1015 | 34.1 | `allocate_and_free_meshes` 19.11, `prepare_erased_assets<WaterMaterial>` 10.85, `submit_pending_command_buffers` 8.43 |
| flight | 342 | 25.8 | upload 16.28, water 8.14, submit 4.39 |
| flight | 220 | 18.1 | upload 12.67, `prepare_assets<GpuImage>` 4.65 |
| walk | 484 | 34.6 | upload 18.22, water 10.37, `update_sparse_buffers` 8.67 |
| walk | 326 | 30.3 | upload 17.49, water 9.36, `update_sparse_buffers` 4.81 |
| walk | 219 | 21.1 | upload 12.96, `prepare_assets<GpuImage>` 4.98 |
| route-walk | (two) | 31.8, 28.1 | the same three systems |

Below those, every one of the top twenty render frames on every fixture is
an ordinary upload frame at 15 to 18 ms with `allocate_and_free_meshes` at
7.7 to 7.9.

## Where the two reads disagree

| Point | Trace | Design read | Believed, and why |
| --- | --- | --- | --- |
| Cost per region upload | 3.9 ms, 6.6 ms/MB, from the span | 3.7 ms, 6.4 ms/MB, from the render reading before and after the route-walk drain | The trace: a span, not a difference. The two agree within 5 %. |
| Why the flight's main thread alternates between 6 to 8 and 12.6 to 15.3 ms | The render world's upload task runs nested inside `update_mesh_previous_global_transforms` in 152 of 973 frames | Left unexplained | The trace. The nesting is in the spans, and `bevy_tasks` 0.19.1 makes it possible: a scope waiting on its own tasks runs `executor.run(..)` on the shared global executor (`execute_global_scope`), so it picks up whatever task is queued, here the render world's system. |
| Collection order | West to east (coordinate order) | At positive world x every coarser LOD sorts ahead of LOD 0, so the ground under the camera is built last | Both, the design read more precisely. `RegionCoord` derives `Ord` on `(x, z, lod)` with `x` in units of the region's own size, so a coarse region's `x` is near zero: at positive x it sorts first, at negative x LOD 0 does. Priority depends on the quadrant. The flight stays at positive x, which fits the probes reading "0 live" for the whole flight. |
| The async pool | 79 % busy (3.16 thread-seconds a second, measured); nothing in the frame waits on it | About 4 thread-seconds a second (estimated), and a latency problem for pipeline compiles and near regions | The measurement for the load. Both agree it is latency, not frame time. It matters more after the upload fix: at real frame rates the upload budget stops being the bound and the pool's 86.6 meshes a second (against 93 wanted at 200 m/s) becomes it. |
| A slimmer terrain vertex | About 260 KiB a region: x and z from the vertex index, UV from x and z, positions down to a height | 393 KB: `POSITION` stays `f32x3`, 6 attributes and 76 B, `u16` indices | The design read's for the `Mesh` path. Under Bevy's slab allocator an indexed draw adds the slab's `base_vertex`, so `@builtin(vertex_index)` is slab-relative, and the shader cannot rebuild x and z without a per-region record. The trace's layout is the right one for the storage-buffer path, where the shader has that record. Neither touches `collider_matches_mesh.rs`, which compares `RegionMesh` positions on the CPU. |
| Attributes per vertex | 12 | 13 | 13, from the code: position, normal, UV, material, morph, tint, cover, sky, sky parent and four weight words, 108 B. Immaterial to the fix. |
| When the walker stops | About 30 s in | About 16 s in | The same place, (-494, -328); the logs and the trace start their clocks at different moments. Neither found the cause. |
| Colliders | Build the shapes on the task, a 1 ms time budget, a despawn budget, a radius around bodies | A reach around characters, one shape per species and variant, hulls off the main thread | Both; they compose. The shape cache and the budgets first, since they hold for the server too; the reach rule only pays on a flight or flycam with no character and has to be squared with the server (below). |
| The 30 to 40 ms render frames | Upload at 16 to 19 ms with the water material at 8 to 10 ms, inferred to be slab growth | Not addressed (notes about 0.5 GB of slab) | Not decided. Bevy's `SlabAllocatorSettings` default to 1 MiB minimum, 512 MiB maximum and a 1.5 growth factor, so 470 MB of terrain vertices does pass through about fifteen growths, but steady flight keeps the resident set flat and the hitches recur two or three times a minute. Measure it first (the last section). |
| The rebase | No outlier main or render frame at any of the six rebases on the flight | Touches everything; unmeasured | The trace: it measured. Not a streaming cost today. |

## The costs, ranked

By measured ms per frame (traced means; the untraced split above gives the
p95 effect):

| # | Cost | Thread | Flight | Walk | Route-walk | Effect on p95 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Terrain region upload (`allocate_and_free_meshes`) | render | 7.75 | 2.32 | 3.43 | +7.7 to 7.9 ms on every upload frame: all of the flight, 28 % of walk, 41 % of route-walk |
| 2 | The same task stolen by `update_mesh_previous_global_transforms` | main | 1.19 (woodland 0.13) | 0.48 | 0.61 | +8 ms on 16 % of flight frames; walk main p95 7.2 to 15.2 on upload frames |
| 3 | Colliders: insert, its commands and despawns | main | 1.48 | about 0 | small | bursts of 6 to 9 ms |
| 4 | Streamers re-deriving unchanged state (foliage dispatch and collect, `select_regions`, `diff_regions`) | main | 1.22 | 1.33 | 1.59 | 1.16 at rest |
| 5 | Foliage collect's commands and apply | main | 0.38 | under 0.07 | 0.17 | collect to 6.6 ms, apply to 3.1 |
| 6 | Water material re-prepared every frame | render | 0.17 | 0.12 | 0.13 | rides the worst frames at 8 to 10 |
| 7 | Bounds over every vertex of each new tree level (`calculate_bounds`) | main | 0.13 | 0.16 | about 0.1 | up to about 3 ms on a 600-spawn frame (estimate) |
| 8 | Meshes built and dropped, queues without a cap | pool | 0.70 thread-s/s | 0 | 0 | none; RAM to about 650 MB |

By hitch size:

| Hitch | Size | How often | Made of |
| --- | --- | --- | --- |
| Render frame of 30 to 40 ms | 15 to 25 ms over an upload frame | 2 or 3 a minute on every moving fixture | upload 16 to 19 ms, water material 8 to 10, `submit_pending_command_buffers` or `update_sparse_buffers` 4 to 8.7 |
| Main frame of 18 to 24 ms (flight) | 10 to 16 ms over p50 | the 20 worst flight main frames | stolen upload 8 ms plus `collect_colliders` 2 to 7 ms |
| Main frame of 17 to 24 ms (walk) | 8 to 13 ms | most upload frames | stolen upload 7.8 to 13.2 ms |
| Render upload frame | +7.7 ms | every frame with terrain | two regions through Bevy's interleave |
| Collider despawn burst | 5.9 ms | each 32-region unload on the flight | 299 collider entities removed in one frame, no budget |
| Foliage collect overrun | 6.6 ms + 3.1 ms apply | a few a minute | the 1.5 ms budget checked between cells, 600 spawns allowed |

### 1. Terrain region uploads through Bevy's mesh path

Diagnosis. `RegionMesh::into_mesh` (`crates/strata-terrain/src/mesh.rs`)
makes a `RENDER_WORLD` `Mesh` with 13 vertex attributes and `u32` indices.
Bevy's `allocate_and_free_meshes` copies it into the slab with
`Mesh::write_packed_vertex_buffer_data`, one attribute of one vertex at a
time, 58 000 writes of 4 to 16 bytes at a 108-byte stride into mapped
staging memory. That is 6.6 ms a megabyte, the same constant ADR 0005
measured for grass at about 5. The main thread's side (`into_mesh` and
`Assets::add`) is 0.035 ms a mesh; all of the cost is the render thread's
copy. `collect_meshes` checks `uploaded >= max_upload_bytes_per_frame`
before each insert, so the 1 MiB budget admits a second mesh (1.18 MB in
all), and a budget-bound frame always uploads exactly two.

Fix. Two steps that share their work. First, slim the vertex on the `Mesh`
path: `u16` indices (4 485 is under 65 536) and the six-attribute, 76-byte
layout (position `f32x3`; UV `f32x2`; the four weight words as one
`uint32x4`; one `uint32x4` of slope, sky, sky parent and an
octahedral normal; parent height and parent normal; tint and cover as
`f16x4` in one `uint32x4`), unpacked in `terrain_common.wgsl`, with the
budget check made honest (`uploaded > 0 && uploaded + size > budget`) so
the two packed regions that fit in 1 MiB are still two and the saving is
not spent on a third. Second, take terrain off the `Mesh` path, as grass
and far trees already are: the meshing task packs the region into a byte
record of about 60 bytes a vertex; the main thread writes it with
`RenderQueue::write_buffer` into a pool of equal-size slots in one storage
buffer (every region is the same size, so no fragmentation), freed slots
wait a few frames as `GrassStore`'s do; every region draws one shared
grid-and-skirt index mesh with its slot in `MeshTag`; the vertex shader
rebuilds x and z from the vertex index and the region's step and origin in
a per-slot record, and UV from x and z. The `Aabb` and `NoAutoAabb` the
collector already inserts keep culling as it is. Do not just lower the
byte budget: the cost per byte is the problem, and at a real frame rate
the same per-frame budget is still 7.8 ms on every upload frame.

Saving. Slimming: from 58 000 scattered writes to 27 000 and from 589 KB
to 393 KB a region, so about 1.8 to 2.6 ms a region (by writes or by
bytes) instead of 3.9: 2.6 to 4.2 ms off every upload frame, and the
stolen task on the main thread shrinks with it. Storage buffer: a
`write_buffer` is a memcpy into the staging belt, about 0.1 to 0.2 ms a
megabyte, so about 7.5 ms off every upload frame against today; flight
render p50 from 11.9 to about 4.5 to 5, walk and route-walk render p95
from 14.6 to about 7. With terrain out of the slab, the slab no longer
holds 470 MB and stops growing under it. The upload budget can then be
counted in bytes at memcpy speed (4 MiB a frame is under 1 ms), so the
first fill takes seconds instead of 31 to 37, and throughput stops being
tied to the frame rate.

Risk. Slimming: medium-low. `terrain.wgsl`, `terrain_prepass.wgsl` and
`terrain_common.wgsl` change, and the `f16` tint and cover and the
octahedral normals must hold the look fixtures; `RegionMesh` and its tests
do not change. Storage buffer: medium-high. All three terrain vertex paths
are rewritten, the morph and the sky terms read from the record, a slot
allocator is new, and the buffer reaches about 270 to 300 MB at 1 100
resident regions. Bevy takes the adapter's limits by default
(`WgpuSettingsPriority::Functionality`), so wgpu's 128 MiB default binding
limit does not apply on this card, but a weaker adapter needs the slots
paged (one buffer per band of LODs).

### 2. The upload task stolen by the main thread

Diagnosis. `bevy_pbr::prepass::update_mesh_previous_global_transforms`
(PostUpdate, main world) does a `par_iter_mut` and waits for it. While it
waits, its scope runs the shared compute pool's global executor, and with
pipelined rendering the render world's `allocate_and_free_meshes` is a
task on that same executor, so the main thread runs it. 152 spans of over
5 ms on the flight carry the upload nested inside them at a mean of 7.74
ms; the system's own work is under 0.1 ms.

Fix. Make the render-world task short (cost 1). Nothing in Bevy 0.19.1
keeps one world's executor off the other's tasks, and a system that runs
for under a millisecond does no harm when it is stolen.

Saving. Main p95: walk 9.75 to about 7.2, route-walk 9.93 to about 7.3,
flight 13.5 to about 9 (the colliders remain). Half of that with the
slimmed vertex alone.

Risk. None of its own.

### 3. Collider bursts on the flight

Diagnosis. `crates/strata-physics/src/terrain.rs`: `collect_colliders`
inserts up to 16 heightfields and 256 trunks a frame, and builds every
trunk's shape on the main thread in `spawn_trunk` (`flora.rs`), with a
`Collider::convex_hull` per hull part of every rock and cliff instance,
though the shape depends only on species and variant (the yaw goes on
`Rotation`). Avian's insert hooks follow in the commands. `dispatch_colliders`
despawns every collider of an unloaded region at once. By regression a
heightfield is about 0.18 ms plus 0.06 of commands, a trunk 0.015 plus
0.025, a removal 0.017; `Collider::heightfield` and the spawn are only
0.037, so the trunk shapes and avian's hooks are the bulk. Finished
colliders are taken in `Entity` order, not by distance.

Fix. Build every `Collider` value (heightfield, cylinders, capsules, hull
compounds) on the existing task, and cache one shape per (species,
variant), cloned per instance (a `SharedShape` is an `Arc`). Put the
collector on a time budget of about 1 ms, nearest a character or the
observer first, instead of counts. Queue despawns and take about 64 a
frame; a retired collider far from everything does no harm for a few
frames. Later, and only on a client: build terrain and trunk colliders
within a reach of bodies that need them (the local player's
`CharacterController`, projectiles) instead of every LOD 0 region of the
camera, which removes all of it on a flight or flycam with no character.

Saving. Flight main mean about 1 ms; the worst flight main frames from
about 23 ms to about 15 with cost 2 also gone, and to about 9 to 10 with
the budget; the 5.9 ms despawn burst to under 1.5.

Risk. Medium-low for the budgets and the cache: the server uses the same
module, and a player must never stand where its collider has not been
built, so the order must put the regions around every character first.
The reach rule is medium: the server keeps building by interest, client
and server must agree wherever both have a character, and a projectile
fired far away needs ground under it.

### 4. Streamers re-deriving unchanged state

Diagnosis. `crates/strata-foliage/src/streaming.rs`: `dispatch` rebuilds
and sorts the ring (`select_cells`, about 2 400 to 3 249 cells) every
frame; `collect` clones `wanted`, calls `with_children` on every resident
wanted cell, and recomputes the `FoliageStats` sums over every resident.
`crates/strata-stream/src/lib.rs`: `select_regions` walks the quadtree
(about 1 100 nodes, a sort and two `HashSet` rebuilds) and `diff_regions`
rebuilds the wanted set and an ancestor map. At the settled woodland that
is 0.419 + 0.414 + 0.169 + 0.162 = 1.16 ms a frame with nothing moving.

Fix. Foliage: re-select only when the eye has moved about 8 m or crossed a
cell edge, or the heading has turned; keep a dirty set of residents with
pending work and walk only that, gated by the existing `Checked` slack;
keep the stats as running totals. Regions: skip `select_regions` while no
observer has moved more than about 1 m and `DetailZones` is unchanged; run
the unload pass of `diff_regions` only when `WantedRegions` changed or a
`RegionReady` arrived, and keep the ancestor map between frames.

Saving. About 0.9 ms a frame at rest, 0.5 to 0.8 moving.

Risk. Low. Plant-level crossfades depend on the camera every frame; the
`Checked` slack already bounds how stale they may be, and the gate must use
the same slack. The stream crate's no-hole tests run under a fake builder
and must pass unchanged.

### 5. The foliage collector's overruns

Diagnosis. `collect` checks `Budget::time_left` only between cells and
between atomic installs, and its spawn and despawn budgets are 600 entities
a frame each (`FoliageConfig`, `crates/strata-foliage/src/lib.rs`). Collect
reaches 6.3 to 6.6 ms on every fixture against a 1.5 ms soft budget, and
its apply reaches 3.1 ms on the flight.

Fix. Spawn and despawn budgets to about 150 a frame; check the time inside
`update_plant_levels` per plant, not per cell; count the previous frame's
measured apply time against this frame's budget so collect and apply
together stay near 1.5 ms. Give each new tree level its bounds from the
prop mesh once, when `TreeAssets` is installed, and spawn it with that
`Aabb` and `NoAutoAabb` as the far trees and terrain already are
(`spawn_tree_level` in `tree.rs` inserts `Mesh3d` and no `Aabb`).

Saving. Bursts from 6.6 + 3.1 ms to about 2; `calculate_bounds` 0.12 to
0.16 ms of mean; the mean of collect itself is unchanged.

Risk. Low. A fast camera waits longer for near levels; the lead (8 m plus
a quarter second of speed) may want raising for the flight.

### 6. The water material, re-prepared every frame

Diagnosis. `update_water` (`crates/strata-terrain/src/water.rs`) writes
the material's params whenever they change, and the elapsed time is one of
them, so `prepare_erased_assets<WaterMaterial>` re-prepares the material on
every frame: 0.12 to 0.17 ms a frame, and 8 to 10 ms in exactly the frames
where the upload runs at 16 to 19 ms. That coincidence suggests the two
are waiting on the same device or queue work; it is not measured.

Fix. Move the time-varying terms (time, the origin wrap, the wind) into a
global uniform, as the foliage wind already is, so the material is
prepared once.

Saving. 0.12 to 0.17 ms a frame, and it takes the water out of the worst
frames whatever their cause.

Risk. Low; the look fixtures with water (coast, city-bay) must not change.

### 7. The terrain queue: no cap, coordinate order, late drops

Diagnosis. `dispatch_meshing` spawns a task per `RegionLoad` with no cap
on tasks in flight. `collect_meshes` sorts finished tasks by `RegionCoord`
(derived `Ord`, so by x index, then z, then LOD; see the disagreements) and
takes the first two. A finished mesh whose region has left the wanted set
waits until its `RegionUnload`. On the flight 450 finished meshes wait on
average, 1 104 at most, about 0.6 MB each, and 61 a second are dropped
unused: 0.70 of the pool's 4 thread-seconds a second. The near ground is
not first: probes need a built LOD 0 region within 192 m and found none
for the whole flight.

Fix. A pending queue of loads ordered by the observers' current distance,
a split's four children as one unit (the no-hole rule keeps the parent
until all four are ready, so a split that is three quarters done holds two
levels); start tasks only while in flight plus finished-and-waiting stays
under about two seconds of upload throughput (16 to 32 today); upload the
nearest finished first, recomputed each frame from `Observers`; drop a
waiting mesh as soon as its region leaves `WantedRegions`. Ties break by
coordinate, which keeps two runs with the same history identical.

Saving. No frame time. About 0.7 pool thread-seconds a second and up to
about 650 MB of RAM on the flight, near ground built first, fewer parent
regions held by the no-hole rule (the design read saw 1 479 resident
against a steady wanted set of 783 to 850).

Risk. Low. The stream crate's no-hole tests and the terrain crate's
headless tests cover the order.

### 8. The shared pool and duplicate sampling

Diagnosis. All streaming (terrain, colliders, foliage, probes, fog) and
Bevy's pipeline compiles share the async pool's four threads, first in,
first out, on a 32-thread CPU. At LOD 0 the same 1 m lattice is sampled by
the mesh task, the collider task and the foliage placement, and the tree
placement runs twice (physics and foliage): about 24 000 samples where
4 761 are distinct, about 25 ms of pool per LOD 0 cell. Far foliage cells
(about 2 100 of 2 290) sample the full lattice (about 9 600 points, 13 to
17 ms) to read only the tree positions their impostors need. Once uploads
are cheap, this is what bounds the flight at real frame rates: the pool
finished 86.6 meshes a second against the 93 the selector wants at 200 m/s.

Fix. A streaming pool of its own (a resource with a fixed number of named
threads, six to eight, keeping the machine-wide total under the 16 the
tests use), or `async_compute` raised in `TaskPoolPlugin`. A short-lived
cache of the 69 by 69 sample block per LOD 0 region, filled by the mesh
task and read by the collider and foliage tasks, and the cell's
`CellFlora` shared between foliage and physics (the server computes its
own). A lazily filled lattice in `HeightGrid` so a far cell samples only
the points its candidates touch.

Saving. Pool time and latency, not frame time: about 20 ms of pool per
LOD 0 cell from the cache, 2 to 3 times fewer samples per far cell.

Risk. Medium. Determinism must hold bit for bit (same function at the same
points; the seed-42 placement snapshot and `collider_matches_mesh` must not
move); the cache needs a Bevy-free home below the physics layer; more busy
threads on a CPU that fails under heavy load.

### Not a streaming cost today

The rebase (no outlier frame at the flight's six rebases; `sync_transforms`
0.07 to 0.08 ms), probes (at most a bake a second at 9.5 ms on the pool,
none in the top sixty systems) and the fog clipmap (not registered in any
fixture). The walk's scene costs (the city's shadow-caster visibility,
`cascade_trim`, about 1.5 ms of main) are real but are not streaming.

## The plan

Ordered by what each phase removes per unit of risk, with the bench made
honest first because every later acceptance number depends on it. Phases
in different crates run in parallel; phases in the same crate are
sequenced. Measurements are serialised: one game on the machine at a time,
never while another agent compiles, and each crate's tests run one binary
at a time under the 16-thread cap.

| Phase | What | Files | Saving (estimate) | Risk | Lane |
| --- | --- | --- | --- | --- | --- |
| 0 | Moving fixtures that measure moving | `tools/bench.py`, `crates/strata-client/src/stats.rs`, `crates/strata-client/src/flight.rs`, `apps/game/src/main.rs`, `apps/game/src/route.rs` | none; walk and route-walk numbers become meaningful | low | first, alone |
| 1 | Terrain queue: capped, nearest first, early drops | `crates/strata-terrain/src/lib.rs` | 0.7 pool thread-s/s, up to 650 MB RAM, near ground first | low | A |
| 2 | Slim terrain vertex on the `Mesh` path | `crates/strata-terrain/src/mesh.rs`, `material.rs`, `lib.rs` (budget check), `assets/shaders/terrain.wgsl`, `terrain_prepass.wgsl`, `terrain_common.wgsl` | 2.6 to 4.2 ms render per upload frame; main stalls halved | medium-low | A, after 1 |
| 3 | Collider shapes off the main thread, budgets, nearest first | `crates/strata-physics/src/terrain.rs`, `flora.rs` | flight main mean about 1 ms; bursts 6 to 9 ms to about 1.5 | medium-low | B, parallel |
| 4a | Region selection only on change | `crates/strata-stream/src/lib.rs`, `select.rs` | about 0.3 ms main at rest | low | C, parallel |
| 4b | Foliage on change, tighter collect budgets, tree-level bounds | `crates/strata-foliage/src/streaming.rs`, `lib.rs`, `tree.rs` | about 0.6 ms main at rest; collect bursts 6.6 to about 2 | low | D, parallel |
| 5 | Terrain from a storage buffer | `crates/strata-terrain/src/lib.rs`, `mesh.rs`, `material.rs`, a new `store.rs`, the three terrain shaders | the rest of cost 1: about 7.5 ms render per upload frame against today, the stolen task gone, terrain out of the slab | medium-high | A, after 2 |
| 6 | Water params in a global uniform | `crates/strata-terrain/src/water.rs`, `assets/shaders/water.wgsl` | 0.12 to 0.17 ms render; water out of the worst frames | low | A, any time after 1 (same crate) |
| 7 | Streaming pool and shared samples | `crates/strata/src/plugins.rs` or a pool resource in `crates/strata-sim`, `crates/strata-flora/src/lib.rs`, `crates/strata-foliage/src/streaming.rs`, `crates/strata-physics/src/terrain.rs`, `crates/strata-terrain/src/lib.rs` | latency and throughput at real frame rates, not frame time | medium | last, alone |

Each phase records its numbers with `just bench --xvfb --record "<label>"`
on the three moving fixtures and the settled woodland, and the look
fixtures with `just look --compare`. The acceptance numbers below are work
time under Xvfb, against the Phase 0 baseline where the phase changes the
fixture's meaning and against today's untraced runs where it does not.

### Phase 0: moving fixtures that measure moving

What. Hold each moving fixture's observer at its start until the terrain
is idle (`LoadedRegions` has nothing pending and no finished mesh waits)
and the foliage streamer is idle, then start the motion, and measure from
a few seconds after it starts: a terrain-idle end for `BenchRecorder`
beside the foliage one, and a "go" the flight path, the auto-walk and the
route wait on. Find why the auto-walk stalls at (-494, -328) (a trunk or
rock collider across its path is the likely cause) and move its start or
give it a route, and make a walker that has not moved 5 m in 5 s fail the
run. Add to the `--stats` and bench line: terrain meshes waiting,
`TerrainStats::cancelled`, uploads this frame. Record a new section,
"Streaming baseline".

Acceptance. walk: observer moves at least 1 m/s over the whole window;
render p95 about 6.8 and main p95 about 7.2 (the post-fill figures above)
plus whatever its real streaming adds. route-walk: render p95 about 13.0,
main p95 about 8.0 (its post-fill figures). flight: within 0.5 ms of
today's p95s (it never settles; only its start changes).

Must not change. No game behaviour outside the bench flags; the static
fixtures' warm-up is untouched.

### Phase 1: the terrain queue

What. Cost 7's fix in `crates/strata-terrain/src/lib.rs`: the pending
queue by distance with splits as units, the in-flight cap, nearest-first
upload, early drops. The upload budget stays as it is in this phase.

Acceptance. flight: finished meshes waiting from 431 mean to at most 40;
dropped finished meshes from 61 a second to under 5; terrain pool demand
from 0.99 to about 0.4 thread-seconds a second; resident regions towards
the wanted set (from about 1 090 to under 950); the probe log shows live
probes along the flight. Thread p95s unchanged within 0.5 ms (this phase
is not a frame-time phase).

Must not change. The no-hole rule (the stream crate's tests and the
terrain crate's headless tests, run one binary at a time); the inserted
order is deterministic for a given history.

### Phase 2: the slim terrain vertex

What. The first step of cost 1: `u16` indices, the six-attribute 76-byte
layout, the honest budget check, unpacking in `terrain_common.wgsl` used
by both the main and prepass vertex shaders. Keep the CPU packing in a
function the storage path (phase 5) will reuse, and the WGSL unpacking in
functions it will call with a record instead of attributes.

Acceptance. Traced `allocate_and_free_meshes` per region from 3.9 ms to at
most 2.6. flight: render p50 from 11.9 to at most 9.5, render p95 from 13.0
to at most 10.5, main p95 from 13.5 to at most 11.5, worst render frame
under 25. route-walk (Phase 0 baseline): render p95 from about 13.0 to at
most 10.5. Terrain meshes per frame on a budget-bound frame still 2.

Must not change. The four look fixtures (`just look --compare` against the
pre-phase capture, no visible difference in the ground's tint, cover,
normals or morph); `collider_matches_mesh` and the terrain crate's mesh
tests (`RegionMesh` is untouched); the world's bits.

### Phase 3: colliders

What. Cost 3's fix without the reach rule: shapes built on the task, one
shape per (species, variant), the 1 ms time budget ordered by distance to
characters first and the observer second, despawns at about 64 a frame.

Acceptance. flight: `collect_colliders` plus its commands at most 2 ms in
any frame (from 6.87 + 3.00), `dispatch_colliders` commands at most 1.5
(from 5.88), main mean down about 1 ms, worst main frame from 27.1 to under
20 (under 12 once phase 5 is in). walk and route-walk: no change beyond
noise.

Must not change. The server's colliders (same shapes, same places; the
physics crate's headless tests and `just server-check`), the character's
ground on spawn and at a sprint (the route's grounding: no fall, no
correction over the route), and the layering rule (the physics crate gains
no render dependency).

### Phase 4a: region selection only on change

What. Cost 4's fix for `crates/strata-stream`: skip `select_regions`
under 1 m of movement with zones unchanged; `diff_regions`' unload pass
only on a change; the ancestor map kept.

Acceptance. woodland: `select_regions` + `diff_regions` from 0.33 ms to
under 0.05; main p95 down about 0.3. route-walk: the two from 0.53 to
about 0.3.

Must not change. The no-hole tests and the selector's own tests; the
server's interest selection (`in_interest` walks the selector per client
every 8 m and must see the same sets).

### Phase 4b: foliage

What. Cost 4's fix for the foliage streamer, cost 5 (budgets of about 150,
per-plant time checks, apply time counted) and the tree-level bounds.

Acceptance. woodland: foliage dispatch + collect from 0.83 ms to under
0.25, main p95 from 5.53 to about 5.0. All moving fixtures: foliage collect
max under 2 ms (from 6.3 to 6.6), apply max under 1 ms, `calculate_bounds`
mean under 0.05.

Must not change. Placement (the seed-42 snapshot); the crossfade bands and
the dwell (the woodland and grove look fixtures); the foliage entity count
at the settled fixtures within 2 %.

### Phase 5: terrain from a storage buffer

What. The second step of cost 1, on top of phase 2's packing: the record
packed on the task, a slot pool in one storage buffer sized from the
region budget up front, one shared grid-and-skirt index mesh, slots in
`MeshTag`, x, z and UV from the vertex index and the slot record, the
upload budget in bytes (about 4 MiB a frame). If the adapter's storage
binding limit is under the pool's size, page the slots by LOD band.

Acceptance. Traced `allocate_and_free_meshes` under 0.3 ms mean on the
flight, `update_mesh_previous_global_transforms` max under 1 ms on every
fixture. flight: render p50 at most 6.0, render p95 at most 7.5, main p95
at most 9.0 (at most 8.3 with phase 3), worst render frame under 16.
walk and route-walk: render p95 at most 7.5, main p95 at most 7.5, no
frame over 33 ms. The first fill around a new camera from 31 to 37 s to
under 10 (pool-bound).

Must not change. The look fixtures, morphing between LODs included; the
`Aabb` per region and frustum culling; the shadows terrain casts; the
world's bits (the GPU record is derived from `RegionMesh`, which does not
change); the layering rule (all of it inside `strata-terrain`, a render
crate).

### Phase 6: the water uniform

What. Cost 6's fix.

Acceptance. `prepare_erased_assets<WaterMaterial>` under 0.02 ms mean with
no frame over 1 ms on the three moving fixtures. If phase 5 is not yet in,
the worst render frame on each moving fixture loses the 8 to 10 ms water
share.

Must not change. The coast and city-bay looks (waves continuous across a
rebase).

### Phase 7: the pool and shared samples

What. Cost 8's fix: the streaming pool, the LOD 0 sample cache with the
shared `CellFlora`, the lazy lattice for far cells. Only after phase 5,
when the pool is the bound at real frame rates.

Acceptance. flight: meshing, collider and placement pool time per LOD 0
cell from about 25 ms to about 8; terrain production at least 100 meshes a
second (above the 93 the selector wants at 200 m/s); foliage placements a
second up at least 1.5 times; thread p95s unchanged within 0.3 ms.

Must not change. The world's bits (the seed-42 placement snapshot,
`collider_matches_mesh`, the worldgen tests); the server path, which must
work with no foliage and no cache filled by a mesher; the layering rule
(the cache lives in a Bevy-free crate or in `strata-sim`, never in a
render crate the physics would have to see); the machine-wide thread count
under the 16 the tests use.

### What every phase must leave as it is

- **The look fixtures**: `just look --compare` before and after each phase
  that touches a shader or a level's lifetime (2, 4b, 5, 6).
- **Determinism of the world**: worldgen, placement and the colliders are
  pure functions of the seed and the library; no phase changes a sample,
  a placement or a collider shape, and the collider-versus-mesh test
  holds bit for bit.
- **The layering rule**: no Bevy below `strata-sim`; `strata-physics`
  and `strata-stream` stay on the non-render allowlist (`just
  server-check`); terrain's GPU store stays in `strata-terrain`.
- **The replication handshake**: nothing here touches
  `strata-net-replicon/src/protocol.rs`, the combat tables or the tree
  library, so the protocol hash does not move. ADR 0002 holds: positions
  stay `WorldPos`, and the storage path derives render positions from the
  region's origin as the `Mesh` path does.

## What to measure first

Where an estimate carries a phase, measure before building it:

1. **The cost of the slim vertex**, before phase 2: in the scratch tree,
   build the 76-byte mesh from the same `RegionMesh` and time
   `allocate_and_free_meshes` per region on the flight. If it lands near
   3 ms (cost by bytes) rather than 1.8 (cost by writes), go straight to
   phase 5 and skip the interim layout.
2. **Slab growth**, before phase 6 is credited with the worst frames:
   count `MeshAllocator` slab reallocations per frame (a wrapper in the
   scratch tree) and join them to the 30 to 40 ms frames. If they line up,
   phase 5 removes them; if they do not, trace the queue between
   `allocate_and_free_meshes` and the water material.
3. **The walk's stall**, in phase 0: log the character's contacts at
   (-494, -328).
4. **Pipeline compiles on the pool**, before phase 7: count compiles still
   queued per second during the first fill.
5. **A flight with the fog on** (`--fog 1`), once: the clipmap's tiles
   share the pool and add a 512 KB texture write a level, and no fixture
   has measured them.
6. **The real display**: one flight at a fixed clock on the desktop after
   phase 5, since at 60 to 120 fps the per-frame budgets pass three to six
   times the work a second and the pool, not the upload, may set the pace.

## Reproducing

Everything is in the session scratchpad under `prof/`
(`/tmp/claude-1000/-home-user-repos-personal-gamedev-2026-september-sci-fi-scrolls/1ad7727a-8df5-4529-918f-e5d85a645fca/scratchpad/prof`):
the instrumented tree and both binaries (`game-trace`, `game-plain`), the
per-frame recorder, the per-run CSVs and `an.json`, and the scripts
(`analyse.py`, `through.py`, `hitch_plain.py`, `nested.py`, `tables.py`,
`instrument.py`). The design read's selector port is `stream-reader/select_sim.py`
in the same scratchpad. The traces themselves (about 19 GB) were deleted
after analysis; `game-trace` with that tree's `bench.py` rebuilds them,
one fixture at a time, about 6 GB each. The per-frame recorder was
throttled by dirty-page writeback while the traces were written, which
made the traced flight's raw 785 ms main frames; its time is taken off the
traced main frames above, and the untraced runs are clean.

## What was built (2026-10-04)

Phases 0, 1, 3, 4a, 4b, 5 and 6, in the lanes of the plan. Phase 2 was
not built on its own: the storage path went in directly and carries phase
2's packing as its record (`strata_terrain::store`), so there is no
interim 76-byte `Mesh` layout. Phase 7 was deliberately not started. Each
phase was measured as an A/B with `tools/bench.py --xvfb`, two runs of the
old binary and two of the new, interleaved, one game on the machine; the
per-system numbers come from the same bench with `--features trace`, over
the last 60 s (100 s for the walks). Phase 6's A/B ran on a tree that
already held the store, so its "before" was the first measurement of
phase 5; phase 5's own acceptance numbers and the final table are from
one pass of the finished tree.

| Phase | What was done | Before | After |
| --- | --- | --- | --- |
| 0 moving fixtures | The moving fixtures hold their observer at the start until the terrain and the foliage are idle, then move and measure from 5 s after (`--bench-warm hold`); the walk starts at (-1200, -200) and walks west, clear of Port Serein's wall; a run under 1 m/s, or under 5 m in any 5 s, fails; the bench line and json carry terrain uploads, tasks, pending regions, cancellations, colliders and the speed | walk render p95 13.28 / 12.88, main p95 8.18 / 7.97, the character standing at the wall; route-walk render p95 13.54 / 14.01; flight render p95 19.61 / 12.50 | walk 11.03 / 11.19 and 7.87 / 7.84 at 5 m/s; route-walk 12.98 / 13.22; flight 12.72 / 12.66: the baseline the later phases compare with |
| 1 terrain queue | Loads wait in a queue ranked by the observers' distance, holes and merges first, a split's children as one unit; at most 16 meshes on the pool or waiting; nearest finished first; a mesh is dropped as soon as its region leaves the wanted set | flight: finished meshes waiting 434 / 536 mean; 61 dropped a second; terrain pool 0.99 thread-s/s; 1 085 / 1 130 regions resident; 23 probe bakes over the flight | 11.7 / 11.5 waiting; 2.1 / 2.4 dropped a second; 0.45 / 0.49 thread-s/s; 979 / 1 004 resident; 368 / 525 probe bakes, so the ground under the camera is built first. Thread p95s within the runs' noise |
| 3 colliders | Every shape built on the task, one hull set per (species, variant) shared by its instances, the collector on a 1.25 ms budget nearest a character first and then the observer, a region next to a character finished whole, despawns at most 64 a frame within 0.5 ms | traced flight: `collect_colliders` with its commands p95 4.15, max 11.3 ms; `dispatch_colliders` commands max 6.0 | p95 1.31, max 1.40 (1.56 in the final tree); despawns max 0.03. Untraced thread p95s within noise |
| 4a region selection | `select_regions` runs only when an observer has moved 1 m or the zones changed; `diff_regions`' unload pass only on a change, its ancestor map kept | `select_regions` + `diff_regions` a frame: woodland 0.37 / 0.41 ms; route-walk while moving 0.53 (p95 0.65 to 0.71) | woodland 0.002; route-walk 0.10 (p95 0.36 to 0.40); the server's interest selection unchanged |
| 4b foliage | The ring re-selected only on a move of 8 m, a cell edge or a turn; a dirty set of residents with work; running stats; spawn and despawn budgets of 150 with per-plant time checks and the last apply counted; tree levels spawned with their prop's `Aabb` | woodland main p50 5.26 / 5.13, main p95 6.01 / 5.93; walk main p95 8.27 / 8.19; route-walk 8.16 / 8.20; flight foliage collect max 6.26, its commands 2.43; `calculate_bounds` 0.13 mean | 4.33 / 4.36 and 5.46 / 5.18; 7.49 / 7.58; 7.32 / 7.64; in the final tree collect max 0.83, commands 1.65, `calculate_bounds` 0.028; foliage entities at the woodland 16 649 before and after |
| 6 water uniform | The water's moving parameters live in a buffer of their own that the render world writes from the extracted frame; the material is prepared once | traced flight `prepare_erased_assets<WaterMaterial>` 0.065 mean, 0.24 max, the material modified 18.5 times a second | 0.003 mean, 0.036 max, never modified; the render-world write 0.015 to 0.019 mean. Two coast captures of one binary now agree to 0.14 (1.96 before): the waves no longer lag the frame by a varying amount |
| 5 terrain store | The meshing task packs a region into 60 bytes a grid vertex; the collector writes it with one `write_buffer` into an equal-size slot of one storage buffer and a 32-byte header beside it; every region draws one shared grid-and-skirt mesh with its slot in `MeshTag`, rebuilt by `terrain_attributes` in `terrain_common.wgsl`; the upload budget is 4 MiB a frame; slots wait three frames before reuse; the store pages at the device's largest binding, a material a page | traced flight `allocate_and_free_meshes` 7.75 mean; the stolen task in `update_mesh_previous_global_transforms` max 8.26; 35.7 regions uploaded a second, two a frame; first fill around a new camera 31 to 37 s | 0.003 mean; max 0.59 (0.46 on route-walk); 97.4 a second, up to 13 a frame, none ever waiting on the budget; first fill about 10 s at the flight and woodland, 13 to 15 s on the walks, bound by the pool |

The upload's cost moved to the main thread and shrank: `collect_meshes` is
0.97 ms mean, 1.68 p95 and 2.38 max on the flight at 5.15 regions a frame
(0.17 mean before, for two). Spans around the writes, in a scratch build,
put 0.183 ms mean, 0.204 p95 and 0.69 max on each region's vertex write
(5 835 writes) and 0.004 on its header: a copy into wgpu's mapped staging
memory at about 0.72 ms a megabyte, slower than the plan's 0.1 to 0.2.
No write waited behind the render thread's queue; phase 4b's 2 to 7.35 ms
stall on a main-thread `write_buffer` did not occur.

The finished tree against the ledger's `Phase 6 render thread`, untraced,
one pass, work time under Xvfb; the walks moved at 5.0 and 9.0 m/s and
the flight at 200 m/s. Every gate passes.

| Fixture | Main p50 ms | Main p95 ms | Render p50 ms | Render p95 ms | Main max ms | Render max ms | GPU ms | Entities |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| flight | 6.21 | 7.43 (13.63) | 5.54 (11.87) | 6.86 (12.95) | 19.3 | 19.5 | 4.84 (3.75) | 4 353 (4 744) |
| walk | 5.15 | 6.38 (8.08) | 6.59 (5.94) | 7.70 (13.50) | 9.1 | 9.7 | 4.24 (2.95) | 25 552 (26 683) |
| route-walk | 5.28 | 6.46 (8.10) | 6.44 (6.22) | 7.89 (13.74) | 10.3 | 12.1 | 4.33 (3.50) | 24 774 (24 248) |
| woodland | 4.16 | 5.08 (5.53) | 6.08 (5.81) | 7.03 (6.69) | 6.2 | 8.2 | 4.47 (4.42) | 20 904 (20 902) |
| city-street | 6.83 | 7.72 (8.31) | 6.79 (6.44) | 7.87 (7.16) | 9.1 | 9.1 | 2.90 (2.91) | 27 792 (27 790) |
| valley | 4.09 | 5.03 (5.99) | 6.25 (6.05) | 7.15 (10.66) | 6.2 | 8.4 | 4.65 (4.49) | 15 128 (15 126) |
| coast | 3.67 | 4.26 (5.47) | 4.07 (4.03) | 4.66 (10.58) | 6.0 | 5.9 | 2.97 (2.85) | 7 258 (7 256) |

The moving rows' old figures are the fixed-warm-up fixtures phase 0
replaced (the walk then stood at the wall), so they compare the frame the
player gets, not the same stretch. Against phase 5's acceptance: the
flight's render p50 (6.0), render p95 (7.5) and main p95 (8.3) are met;
the walks' main p95 (7.5) and "no frame over 33 ms" are met; the walks'
render p95 is 7.70 and 7.89 against 7.5, about the settled woodland's
7.03 plus what streaming adds (the store's tree measured 7.40 to 7.71 in
phase 6's runs); the flight's worst render frame is 19.5 against 16. The
look fixtures (`just look --label "Streaming"`, docs/perf/look-ledger.md)
match `Phase 6 render thread` below the horizon: dawn by 0.93 of 255 mean
absolute difference, canopy and dusk-street by what two runs of one
binary differ by (cloud over the sun), coast by the sea's wave phase
alone (its colour statistics within phase 5 to phase 6's spread, the sand
identical). Paging was checked in a scratch build with 128 MiB pages: 1 045
regions over two pages, none pending, the ground the same as one page.

What remains:

- **Phase 7**, the streaming pool and the shared samples, not started and
  set aside on 2026-10-04: every fixture passes, and a larger pool raises
  the load on a machine that has frozen under it. The first fill is now
  pool-bound (100 to 157 meshes a second while it lasts), and so would be
  a flight at a real frame rate.
- **The main-thread write**, 0.19 ms a region: about 1 ms a frame on the
  flight and up to 3 ms on a frame that takes the collector's cap of 16.
  Either a cap of about 8 a frame, which the flight's 5.15 mean allows, or
  the write moved onto a pool task once the slot is chosen (a wgpu queue
  is shared between threads); needs an A/B either way.
- **The flight's worst frames** (25.7 main and 23.5 render in the recorded
  pass) are a stall of the whole process once every 5.0 s, not streaming
  work: a traced bad frame runs the same systems the same number of times
  as its neighbours, each 4 to 5 times slower, on both threads at once.
  Neither the game nor `tools/bench.py` has a 5 s timer. Another game was
  running on the machine during every measurement of the day; with it
  closed the flight's worst frames were 18.0 and 20.1 main, 16.1 and 16.9
  render, its p95 unchanged (7.14 and 7.23 main, 6.40 and 6.38 render), and
  the 5 s beat was still there. Left as good enough: one frame of 16 to
  20 ms every 5 s at twenty times running speed. The test that would
  place it is a traced settled fixture: the beat there means it comes from
  outside the game, its absence that it scales with streaming load.
- **City-street render p95** rose from 7.16 to 7.87 in this one pass and
  no phase touched the city; it wants an interleaved A/B before it is
  believed.
- **The GPU on the moving fixtures** rose (flight 3.75 to 4.84, walk 2.95
  to 4.24) while the settled fixtures held. The walk is a different stretch
  now and the flight keeps its near ground built, but the store's vertex
  stage reads 15 words a vertex from storage; it wants a GPU A/B at a
  fixed clock on a display.
- **The route-walk's walker** stops against a 2.5 m step about 125 s in
  (the bench measures 100 s); the walk's old stall is gone.
- **Not measured**: a flight with the fog on, and one on the real display
  (what to measure first, 5 and 6). Slab growth (item 2) is moot: terrain
  no longer lives in the slab.
