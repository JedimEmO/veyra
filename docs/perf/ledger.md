# Performance ledger

The numbers each phase of [the world slice plan](../WORLD_SLICE_PLAN.md) exits
on, appended by `just bench --record "<label>"` and never edited afterwards.
A later phase is compared against the last section before it.

The gates are section 3 of the plan: at every fixture p95 at most 8.3 ms and p99
at most 12 ms; in the moving fixtures (`flight`, `walk`) no frame over 33 ms
after the warm-up. `tools/bench.py` holds those. The per-system frame budget is
read from GPU captures by a person and recorded in prose under a section.

Frames are measured after a warm-up of real time while the world streams in;
`Entities` is every entity in the world when the run ends, which is what a
frame's CPU cost scales with.

Sections from 2026-10-03 on measure the settled frame. Until then the warm-up
was ten seconds, and a static fixture went on measuring its far trees filling
in for about 50 s more (`profile-2026-10-03.md`), so its p95 was mostly
streaming; an older section's static rows are not comparable with a newer
one's. Now a fixture that stands still warms up until the foliage streamer is
idle (nothing in flight and nothing placed for two seconds, after at least ten
seconds and at most ninety) and then measures 3000 frames; the moving fixtures
(`flight`, `walk`, `route-walk`) never settle and keep the ten seconds. The
newer tables add `Render p50 ms` beside the p95 (the render thread is the
bound, and its p50 is the settled cost where the p95 is its hitches),
`Foliage entities` (the median of the streamer's resident entities over the
measured stretch) and `Warm-up`, the seconds it took and how it ended:
`settled`, `ceiling` (the streamer never went idle, so the frames include
streaming) or `fixed`. The full per-pass GPU times, batch sets and mesh
uploads behind each row are in its `scratch/bench/<fixture>.json`.

Sections from the "Streaming baseline" on hold each moving fixture at its
start until the terrain is idle as well (no region waiting for a mesh, no
meshing task held) within the same ten to ninety seconds, then start the
motion and measure from five seconds after it: 100 s for the walks, the
flight to its end (65 s). Before, the walks' p95 was mostly their first
terrain fill around the new camera, and the walk's character stood against
Port Serein's wall for nearly all of its run
(`streaming-2026-10-04.md`); an older section's moving rows are not
comparable with a newer one's. `walk` now starts at (-1200, -200) and walks
west. Their `Warm-up` reads `held` with the seconds the motion waited. The
tables add `Main max ms` and `Render max ms` (the worst frame of each
thread's work over the measured stretch), `Terrain uploads a frame` (the mean
of terrain meshes inserted per frame, each about 7.7 ms of the render thread
until the upload is fixed) and, for a moving fixture, `Moved m/s`, the
observer's mean speed; a moving run under 1 m/s, or under 5 m in any five
seconds, fails. The json beside each row also has the terrain tasks held and
regions pending (mean and max), uploads and cancellations a second, and
ground colliders built and collider entities added and removed a second.

## Phase 6 baseline

2026-09-26, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, MSAA 4x (`baseline` look), 10 s warm-up.

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 2901 | 6.69 | 7.99 | 8.76 | 10.2 | 0 | 5.00 | 59208 | ok |
| grove | 2924 | 7.20 | 8.60 | 9.72 | 31.0 | 0 | 5.53 | 88768 | p95_ms 8.60 > 8.3 |
| valley | 2723 | 5.76 | 7.08 | 7.86 | 9.7 | 0 | 4.10 | 42326 | ok |
| coast | 2528 | 4.84 | 5.94 | 6.56 | 11.7 | 0 | 3.54 | 31215 | ok |
| flight | 8424 | 5.56 | 15.68 | 40.08 | 115.1 | 141 | 3.62 | 4792 | p95_ms 15.68 > 8.3; p99_ms 40.08 > 12.0; 141 frames over 33 ms |
| walk | 4638 | 5.27 | 6.57 | 7.23 | 14.2 | 0 | 3.73 | 22434 | ok |

### Carried misses at the baseline

Two fixtures miss a gate before any phase has changed anything. Each is
carried to the phase that owns its cause, and every earlier phase exit must
leave it no worse.

- **grove**, p95 8.60 ms against 8.3. CPU-bound: 88 768 entities, GPU 5.5 ms
  against a 7.2 ms median frame. Most of the entities are tree level meshes
  and impostors, one per level per tree. Owner: Phase 9, which replaces the
  tree runtime.
- **flight**, 141 frames over 33 ms (worst 115 ms), p95 15.68 ms, p99
  40.08 ms. Bisected on the same day: with `--no-foliage` the same flight
  measured p95 9.64 ms, p99 11.37 ms, worst 42.4 ms, 3 frames over 33 ms, so
  foliage streaming causes nearly all of the stalls. Owner: Phase 9 for the
  foliage stalls. The remaining p95 of 9.6 ms is terrain streaming at 200 m/s,
  twenty times running speed. Owner: Phase 7, which raises the cost per terrain
  sample and must pay that back.

## Phase 7 exit: not recorded

2026-09-26. The full bench was not recorded for Phase 7. On this machine
(GNOME on X11, a 144 Hz primary beside two 60 Hz monitors) a run is paced at
60 Hz unpredictably, windowed or fullscreen, and a paced run's percentiles say
nothing about the game. The user chose to skip gating for now. `tools/bench.py`
refuses a paced run instead of recording it, and a scripted game now updates
continuously whether or not its window has focus.

What was measured, on the same day and in unpaced runs:

- **woodland**: GPU 5.05 ms against 5.00 at the baseline, after the terrain
  shader was brought back to the old read count (roughness packed into the
  colour array's alpha, one-biome ground read once). p95 8.5 ms against 7.99:
  the frame is CPU-bound, and erosion's drainage grew about 5% more plants
  there (62 322 entities against 59 208). That is the tree runtime's cost per
  entity, which is Phase 9's.
- **flight without foliage**: p95 7.5 to 7.7 ms against 9.5 for the Phase 6
  build on the same day, inside the gate: the terrain's share of the carried
  flight miss is paid back by the 1 MiB-per-frame budget on terrain mesh
  uploads. Frames over 33 ms: 10 to 15 per run against 8 for the Phase 6
  build, within the spread runs show; the foliage stalls are still Phase 9's.

## Phase 8: capacity test

2026-09-26, RTX 4090, `prop-capacity` in a window, MSAA 4x, one unpaced run
each. It is not a wall-clock gate (see Phase 7). 20 species by 8 variants of a
synthetic 15k-triangle tree (`veyra_flora::capacity`), cut-out leaf cards,
near shadows.

| Range | Trees | Entities | Mesh MB | p50 ms | p95 ms | GPU ms |
| --- | --- | --- | --- | --- | --- | --- |
| mesh levels to 350 m, opaque cards | 4 768 | 28 608 | 443 | 2.38 | 3.09 | 1.02 |
| mesh levels to 350 m, cut-out cards | 4 768 | 28 608 | 443 | 3.57 | 4.56 | 2.38 |
| to 1.6 km, impostors past 350 m | 99 300 | 695 100 | 443 | 6.87 | 8.03 | 2.77 |

## Phase 9: trees

2026-09-27, RTX 4090, i9-13900KF, Linux, 2560x1440 window, release, MSAA 4x, one unpaced run each; not a gate (see Phase 7).

| Fixture | Build | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Entities |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| flight | Phase 8 | | 8.00 | 32.18 | 55.38 | 105.7 | 258 | 4.47 | 4 375 |
| flight | Phase 9, before the scheduler and avian fixes | | 6.33 | 28.78 | 49.28 | 110.8 | 240 | 4.04 | 3 822 |
| flight | Phase 9 | | 5.72 | 9.05 | 10.32 | 33.1 | 1 | 3.97 | 3 854 |
| flight, no foliage | Phase 9, before the fixes | | 5.74 | 7.59 | 11.55 | 58.4 | 13 | 4.22 | 1 897 |
| woodland | Phase 9 | | 9.81 | 12.33 | 13.98 | 44.5 | 2 | 5.80 | 42 237 |

Settled woodland, 3 000 frames, same day, 2560x1374: the Phase 8 build 12.6 ms mean, 8.5 ms GPU, 161 818 entities, 546 956 plants;
Phase 9 13.7 ms mean, 8.6 ms GPU, 64 714 entities, 642 006 plants.

## Phase 10: scatter, understory, rocks and cliffs

2026-09-27, same machine and settings as Phase 9, one unpaced run each; not a gate.

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Entities |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| flight | | 6.27 | 9.26 | 10.44 | 33.1 | 1 | 4.11 | 7 022 |
| woodland | | 10.22 | 12.67 | 13.99 | 49.8 | 3 | 5.65 | 48 188 |

Placement per cell on a worker thread: about 21 ms in the woodland (15 ms at Phase 9).
Library: 16 species, 108 variants, 627 915 near triangles, 130 MB of props and impostors.

## Phase 12 exit

2026-09-27, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, MSAA 4x (`baseline` look), 10 s warm-up.

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3376 | 10.40 | 12.85 | 14.01 | 54.8 | 1 | 5.59 | 78533 | p95_ms 12.85 > 8.3; p99_ms 14.01 > 12.0 |
| grove | 3226 | 9.99 | 12.39 | 13.67 | 39.7 | 2 | 5.77 | 99083 | p95_ms 12.39 > 8.3; p99_ms 13.67 > 12.0 |
| valley | 2985 | 6.04 | 7.56 | 8.35 | 31.6 | 0 | 4.20 | 51380 | ok |
| coast | 2758 | 5.47 | 6.93 | 7.57 | 29.3 | 0 | 3.72 | 42869 | ok |
| flight | 9105 | 6.53 | 9.63 | 10.75 | 30.7 | 0 | 4.03 | 35999 | p95_ms 9.63 > 8.3 |
| walk | 5081 | 6.51 | 7.74 | 8.60 | 12.5 | 0 | 3.77 | 48027 | ok |
| city-street | 3007 | 5.97 | 6.94 | 7.37 | 15.7 | 0 | 3.69 | 44410 | ok |
| city-rooftop | 3039 | 5.73 | 6.78 | 7.37 | 13.4 | 0 | 3.86 | 44408 | ok |
| city-approach | 3160 | 6.81 | 8.04 | 8.45 | 8.9 | 0 | 3.88 | 51042 | ok |
| route-walk | 5167 | 6.88 | 8.34 | 10.30 | 12.6 | 0 | 3.93 | 52996 | p95_ms 8.34 > 8.3 |

Settlement streaming (buildings in within 2.5 km of the city, out past 3 km, one a frame) was added after this table.
Rerun of the fixtures it touches:

| Fixture | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Entities |
| --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 10.14 | 13.14 | 14.75 | 44.6 | 4 | 5.62 | 48 211 |
| grove | 9.67 | 12.33 | 14.09 | 52.2 | 2 | 5.80 | 68 761 |
| city-street | 6.04 | 7.10 | 7.67 | 29.6 | 0 | 3.73 | 44 411 |
| city-approach | 6.83 | 8.05 | 8.43 | 10.4 | 0 | 3.85 | 51 043 |
| route-walk | 7.03 | 8.54 | 10.53 | 40.8 | 1 | 3.95 | 53 078 |

The city's 30 000 far entities cost the woodland almost nothing: its frame is the foliage's.
Content step: cold bake 204 s (two cold runs write identical bytes), warm bake 3 s rewriting nothing.
Server: 0.43 ms mean, 0.82 ms worst tick with two players walking in the city.

## The terraced Port Serein, measured by work time (2026-09-27)

RTX 4090, Linux, 2560x1440, release, MSAA 4x (`baseline` look), 10 s warm-up, on a
virtual display (`just bench --xvfb`). The wall clock there is the virtual display's
readback (about 53 ms a frame), so this section records each frame's **work**
instead: the main world's CPU time (`First` to `Last`), the render world's CPU time
(its `Render` schedule to the end of its render graph, less the swapchain wait) and
the GPU time. A frame can take no less than the largest of the three. At about
19 fps the CPU and GPU idle-clock, so these absolutes run higher than in a focused
window (the valley's GPU time was 4.20 ms there, 7.32 ms here); compare within the
section, not against the tables above.

| Fixture | Main p95 ms | Render p95 ms | GPU ms | Entities |
| --- | --- | --- | --- | --- |
| valley | 5.85 | 8.94 | 7.32 | 21 059 |
| coast | 5.74 | 6.54 | 5.89 | 12 548 |
| city-street | 6.71 | 8.46 | 6.49 | 23 556 |
| city-rooftop | 6.64 | 8.50 | 7.76 | 23 554 |
| city-terraces | 6.98 | 8.57 | 6.99 | 24 673 |

The city costs what a foliage scene costs. Its own share, from the bay with and
without its layout: +16 700 entities, +1.2 ms main, +1.4 ms render, GPU within
noise. A Chrome trace of the same pair puts the difference in Bevy's per-entity
passes: visibility propagation (+0.47 ms summed over threads), mesh bounds
(+0.35), sparse buffer updates (+0.29), the sun's four cascades culling and
batching every piece (about +0.6; shadows reach 20 km), the wireframe plugin's
check (+0.14).

The city spawned 13 260 pieces (9 056 of them level zero, drawn only within
60 m) and 3 435 collision proxies nothing on the client reads. Two changes:
the proxies are despawned on the client, and pieces are streamed by 40 m cell
and band (`Disabled` when their band cannot reach the observer). Interleaved
A/B/C runs, main / render p95 ms:

| Fixture | Before | Streamed | Streamed + static bounds |
| --- | --- | --- | --- |
| city-bay | 7.24 / 8.58 (19 011) | 6.85 / 8.21 (11 117) | 6.98 / 8.21 |
| route-walk | 8.06 / 10.44 (29 510) | 7.52 / 9.96 (21 495) | 7.90 / 10.16 |
| city-terraces | 7.02 / 8.43 (21 239) | 6.93 / 8.47 (15 328) | 7.16 / 8.43 |

Streaming is kept: 30 to 40 % fewer entities and 0.4 to 0.5 ms off both
threads where most of the city is out of band. Giving pieces their bounds at
spawn (so `calculate_bounds` skips them) measured nothing and was dropped. The
worst frame of a still fixture rose once (86 to 115 ms at the bay): the frame
the house file's nine thousand pieces arrive and most are disabled at once.

The `baseline` look the benches pin changed on 2026-09-27 (EV 14.4, sky fill 4.0 instead of
14.2 and 1.0). The fill is a multiplier on the sky's environment light, so no pass was added;
rows before this line were taken under the old values.

From 2026-09-27 the `baseline` look also turns on SSAO with TAA in place of MSAA 4x
(the fidelity plan's grounding decision); rows before this line were taken with MSAA.

### After the fidelity pass (2026-09-27, same conditions: `--xvfb`, work time)

| Fixture | Main p95 ms | Render p95 ms | GPU ms | Entities | Before (streamed) main / render |
| --- | --- | --- | --- | --- | --- |
| city-bay | 7.43 | 11.00 | 5.72 | 14 409 | 6.85 / 8.21 |
| city-terraces | 7.46 | 10.55 | 5.92 | 20 028 | 6.93 / 8.47 |
| city-street | 7.11 | 10.44 | 4.95 | 19 246 | 6.71 / 8.46 |
| route-walk | 7.69 | 10.86 | 5.40 | 24 160 | 7.52 / 9.96 |

The GPU is cheaper (TAA at one sample in place of MSAA 4x, with SSAO), but the
render thread is 2 to 2.5 ms dearer and now over the 8.3 ms gate. Changed at once:
the houses' placed elements doubled (a catalogue of shared parts, about 11 elements a
house), the terrain under the city is held at LOD 0 (288 L0 regions in view instead
of about 120), anti-tiled materials are a second material type (a second set of
render phases and pipelines), and SSAO/TAA add passes. Not yet attributed.

### After the rest of the city (2026-09-27, same conditions: `--xvfb`, work time)

After the remaining kits, the headquarters, four slabs and nine warehouses,
walls with ledges, and about 2 300 planted plants (commit e83781a).

| Fixture | Main p95 ms | Render p95 ms | GPU ms | Entities | After the fidelity pass main / render / entities |
| --- | --- | --- | --- | --- | --- |
| city-bay | 8.39 | 14.13 | 9.25 | 19 631 | 7.43 / 11.00 / 14 409 |
| city-terraces | 8.82 | 16.00 | 10.31 | 30 199 | 7.46 / 10.55 / 20 028 |
| city-street | 8.57 | 15.06 | 8.54 | 29 153 | 7.11 / 10.44 / 19 246 |
| route-walk | 8.55 | 12.90 | 9.07 | 29 332 | 7.69 / 10.86 / 24 160 |

What city-terraces pays for, each row the same view with one thing taken
out, runs interleaved, two to five repeats (render within 0.5 ms run to run,
the GPU within about 1.5 ms):

| Taken out | Render p95 | GPU | Entities |
| --- | --- | --- | --- |
| nothing | 16.2 to 16.9 | 8.5 to 10.6 | 30 199 |
| the planted plants | -2.9 | -1.4 | -8 578 |
| the terrace houses | -1.5 | -1.1 | -7 564 |
| all shadows | -3.8 | -4.6 | 0 |
| shadows cast by city pieces | -2.1 | -0.9 | 0 |
| shadows cast by the terrain | 0 | -3 to -4 | 0 |

The render thread scales with entities, about 0.5 ms per thousand here, and
casting shadows costs it per caster. A planted plant is 3.7 entities (a piece
per mesh level), like a wild tree. Shadows cast only by level 0 of the city
(-0.4), glass and lamps not casting (none), the terrain inside the city's
detail zone not casting (none) and a shadow distance of 2 or 5 km instead of
20 (none) do not move it: the city's casters are its level-0 pieces, and the
terrain's shadow cost is in the near cascades. In the trace,
`check_wireframe_entities_needing_specialization` scans every mesh every frame
for F4 (0.4 ms of the main world).

### Shadows of plants and of the distant city (2026-09-28, `--xvfb`, interleaved A/B)

The machine's GPU times ran 2 to 3 ms higher from this session on than in the
tables above (idle clocks under Xvfb), so only differences within one table
mean anything.

| Change | View | Render p95 | Main p95 | GPU |
| --- | --- | --- | --- | --- |
| Understory (bush, fern, flower, log) casts no shadow, kept (3bd1e2b) | city-terraces | -0.7 | 0 | 0 |
| All foliage near levels cast no shadow | city-terraces | -1.8 | 0 | ~0 |
| Plants' mid and far mesh levels not spawned (upper bound of batching them) | city-terraces | -1.2 | -0.3 | ~0 |
| No city piece casts past 150 m | city-terraces | -0.4 | 0 | 0 |
| No city piece casts past 150 m | city-bay | -1.2 | -1.4 | -0.8 |
| Only the kits (houses, market, cargo, street) stop casting past 150 m | city-bay | -0.35 | 0 | -0.45 |

Casting by bounds size does not work for the city: an element's box is the
box of everything it unions (a house's trim runs plinth to cornice), so 12 %
of pieces measured small. The structure and the landmarks are most of the
city's far shadow cost, and they are the shadows that read from the bay (the
transit deck's, the stairs', the walls').

### The far city as a hierarchy (2026-09-29, `--xvfb`, interleaved, ticket 0002)

The city's farthest level joined into 80 m cells (ashlar's `LodCell`), a
skyline file drawn from 1.7 km off the city's edge to the terrain's 48 km,
and no building entity past 2.2 km. Two sessions the same night, 2 000
frames a run, two runs an arm except where said; render thread p95 / main
thread p95 / entities. `city-far` is new: 2 km out over the water
(`--spawn=1132,786 --camera-altitude 120 --look 0.87`), inside the old spawn
distance and past the new switch.

First session. "Nothing past 250 m" is the build before with every piece of
the city past 250 m taken out, one run: the most a far ring could save.

| View | Before | Nothing past 250 m | Joined cells and skyline |
| --- | --- | --- | --- |
| city-bay | 15.2 / 9.2 / 19 631 | 13.6 / 7.8 / 12 934 | 14.6 / 9.3 / 16 781 |
| city-terraces | 16.2 / 10.4 / 30 199 | 15.8 / 9.4 / 27 679 | 16.0 / 10.0 / 28 600 |
| city-approach | 15.4 / 11.1 / 30 855 | 12.6 / 8.7 / 23 583 | 14.1 / 9.2 / 24 659 |
| city-far | 13.1 / 8.4 / 11 728 | 10.2 / 6.6 / 4 456 | 11.2 / 7.1 / 5 532 |

Second session, after the skyline kept its lit elements and a far city's
pieces were taken out of play, with the cell size as the third arm:

| View | Before | 80 m cells | 160 m cells |
| --- | --- | --- | --- |
| city-bay | 15.1 / 9.5 / 19 631 | 14.5 / 10.4 / 16 592 | 14.6 / 8.8 / 16 245 |
| city-terraces | 16.4 / 11.7 / 30 199 | 15.9 / 9.7 / 28 305 | 16.1 / 10.3 / 28 197 |
| city-approach | 15.4 / 12.2 / 30 855 | 14.2 / 8.9 / 24 661 | 14.1 / 9.1 / 24 103 |
| city-far | 13.0 / 8.4 / 11 728 | 11.1 / 6.9 / 4 494 | 11.1 / 7.0 / 4 494 |

The render thread gains 0.6 ms in the bay, 1.2 on the approach and 1.9 from
2 km; among the terraces the change is inside the run-to-run spread. The
main thread's p95 moves by a millisecond between two runs of one build (the
bay's 8.8 to 10.8), so only the approach's and the far view's gains there,
two to three milliseconds and one and a half, are outside it.

The far level went from 7 272 pieces to 1 040 batches at 80 m and 482 at
160 m, and the two measure the same: once the far ring is a thousand
entities it is no longer what the frame pays for. 80 m is kept, for the
shorter overlap where pieces hand over to cells. Taking the far city's
hidden pieces out of play brought its entities to the bound's (4 494
against 4 456) and did not move its render time; what is left of the gap to
the bound, about a millisecond, is drawing the far city at all: 181 000
triangles of skyline, or 451 000 of cells, in the main pass, the prepass
and the sun's cascades.

Every city view is still over the 8.3 ms gate on the render thread. What
they pay for is near: the pieces of levels 0 and 1 in band (5 000 to 9 000
of them) and the planted plants (ticket 0001).

Cost: the city's files are 132 MB on disk for 88, the joined far level of
the Terraces' houses most of it (its 300 000 triangles are no longer shared
between copies), and the skyline 17 MB.

## The new ground: baseline after ticket 0004 step 2b

2026-09-30, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3868 | 61.63 | 83.22 | 87.31 | 92.6 | 3868 | 12.76 | 7.21 | 14.65 | 45262 | work 14.65 > 8.3 |
| grove | 3870 | 64.08 | 85.89 | 90.82 | 95.4 | 3870 | 13.56 | 7.87 | 15.31 | 69532 | work 15.31 > 8.3 |
| valley | 3846 | 62.25 | 83.80 | 88.19 | 93.9 | 3846 | 13.20 | 7.00 | 10.24 | 40042 | work 13.20 > 8.3 |
| coast | 3845 | 55.65 | 73.04 | 75.99 | 86.5 | 3845 | 10.77 | 6.15 | 9.81 | 9602 | work 10.77 > 8.3 |
| flight | 837 | 71.54 | 76.53 | 79.10 | 94.6 | 837 | 18.44 | 13.48 | 13.24 | 5602 | work 18.44 > 8.3 |
| walk | 5898 | 62.50 | 69.57 | 82.12 | 111.7 | 5898 | 10.90 | 8.44 | 14.77 | 30468 | work 14.77 > 8.3 |
| city-street | 3897 | 61.64 | 73.56 | 84.98 | 94.0 | 3897 | 11.06 | 8.97 | 15.29 | 30731 | work 15.29 > 8.3 |
| city-rooftop | 3896 | 64.56 | 74.19 | 88.63 | 96.2 | 3896 | 14.58 | 8.68 | 14.52 | 30116 | work 14.58 > 8.3 |
| city-approach | 3894 | 67.20 | 74.77 | 79.89 | 99.8 | 3894 | 15.45 | 8.84 | 16.23 | 48862 | work 16.23 > 8.3 |
| route-walk | 5893 | 61.40 | 80.95 | 87.22 | 103.7 | 5893 | 11.35 | 9.75 | 16.41 | 47739 | work 16.41 > 8.3 |
| city-bay | 2899 | 62.37 | 69.65 | 85.41 | 97.5 | 2899 | 12.90 | 8.49 | 14.56 | 19343 | work 14.56 > 8.3 |
| city-terraces | 2902 | 64.37 | 72.08 | 89.83 | 112.6 | 2902 | 13.92 | 9.18 | 16.21 | 33001 | work 16.21 > 8.3 |

This is a new baseline and not a comparison. Ticket 0004 moved the ground
(the ranges nearer and running as lines, the climate 56 km south), so every
fixture looks at another place than it did, and three of the study scenes
and the flight were re-sited: no row above this section is comparable with
one below it. The city's own ground and buildings did not move; what stands
round them did. The country round Port Serein is forest where it was sand,
and the city's views carry its trees: city-approach holds 48 862 entities
where it held 24 661, city-street 30 731 for 19 246.

Every fixture is over the gate. The render thread is the bound everywhere
but the flight and the valley, where it is the GPU: the flight's 18.4 ms
is the highest GPU time the ledger has, over ground that is now mountain
for much of its length.

## Phase 1 the sun

2026-10-02, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3864 | 61.63 | 75.87 | 89.77 | 99.7 | 3864 | 12.33 | 7.59 | 15.34 | 56598 | work 15.34 > 8.3 |
| grove | 3872 | 63.13 | 86.21 | 91.29 | 98.9 | 3872 | 13.22 | 7.92 | 15.50 | 69198 | work 15.50 > 8.3 |
| valley | 3846 | 62.14 | 84.27 | 89.11 | 94.0 | 3846 | 12.97 | 7.03 | 10.49 | 39468 | work 12.97 > 8.3 |
| coast | 3848 | 55.63 | 73.73 | 76.61 | 85.4 | 3848 | 10.98 | 6.38 | 10.14 | 9394 | work 10.98 > 8.3 |
| flight | 869 | 70.03 | 79.62 | 81.75 | 126.5 | 869 | 13.35 | 13.80 | 13.55 | 6438 | work 13.80 > 8.3 |
| walk | 5895 | 62.68 | 70.09 | 83.67 | 94.3 | 5895 | 11.39 | 8.86 | 14.84 | 40215 | work 14.84 > 8.3 |
| city-street | 3895 | 61.78 | 75.76 | 88.67 | 98.4 | 3895 | 11.18 | 11.16 | 15.50 | 40884 | work 15.50 > 8.3 |
| city-rooftop | 3894 | 63.42 | 83.18 | 90.08 | 94.8 | 3894 | 13.53 | 8.91 | 14.88 | 39759 | work 14.88 > 8.3 |
| city-approach | 3899 | 68.14 | 75.17 | 90.80 | 106.5 | 3899 | 16.45 | 9.07 | 17.05 | 51652 | work 17.05 > 8.3 |
| route-walk | 5890 | 65.54 | 77.26 | 91.78 | 101.0 | 5890 | 12.66 | 9.57 | 15.63 | 51613 | work 15.63 > 8.3 |
| city-bay | 2893 | 61.93 | 70.42 | 82.75 | 97.3 | 2893 | 12.79 | 8.81 | 14.59 | 24723 | work 14.59 > 8.3 |
| city-terraces | 2897 | 63.27 | 71.69 | 89.60 | 98.2 | 2897 | 13.16 | 9.38 | 15.51 | 43449 | work 15.51 > 8.3 |

## Phase 2 the air

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3861 | 63.66 | 86.03 | 90.88 | 100.7 | 3861 | 13.01 | 7.78 | 15.27 | 56614 | work 15.27 > 8.3 |
| grove | 3872 | 64.17 | 86.78 | 90.77 | 110.3 | 3872 | 13.29 | 8.30 | 15.64 | 69217 | work 15.64 > 8.3 |
| valley | 3849 | 62.87 | 84.72 | 89.18 | 98.9 | 3849 | 12.94 | 7.16 | 10.71 | 39481 | work 12.94 > 8.3 |
| coast | 3851 | 55.76 | 73.97 | 76.86 | 98.3 | 3851 | 11.06 | 6.59 | 10.25 | 9406 | work 11.06 > 8.3 |
| flight | 847 | 72.09 | 79.45 | 90.61 | 119.5 | 847 | 16.79 | 14.24 | 13.77 | 6437 | work 16.79 > 8.3 |
| walk | 5893 | 62.33 | 69.16 | 82.43 | 106.7 | 5893 | 10.93 | 8.95 | 14.99 | 40257 | work 14.99 > 8.3 |
| city-street | 3893 | 62.40 | 77.82 | 87.52 | 95.2 | 3893 | 11.34 | 11.62 | 15.94 | 40904 | work 15.94 > 8.3 |
| city-rooftop | 3894 | 64.15 | 84.98 | 90.86 | 101.6 | 3894 | 13.74 | 9.09 | 14.73 | 39779 | work 14.73 > 8.3 |
| city-approach | 3900 | 68.60 | 75.67 | 89.05 | 114.7 | 3900 | 16.23 | 9.45 | 17.37 | 51667 | work 17.37 > 8.3 |
| route-walk | 5897 | 65.53 | 85.84 | 92.79 | 103.9 | 5897 | 12.81 | 9.76 | 15.96 | 51625 | work 15.96 > 8.3 |
| city-bay | 2894 | 62.29 | 71.70 | 84.16 | 92.3 | 2894 | 12.28 | 9.03 | 14.61 | 24735 | work 14.61 > 8.3 |
| city-terraces | 2897 | 63.71 | 71.48 | 90.60 | 100.2 | 2897 | 12.98 | 9.72 | 15.95 | 43461 | work 15.95 > 8.3 |

## Phase 3 the shade

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3861 | 64.35 | 86.55 | 91.23 | 105.7 | 3861 | 13.56 | 7.76 | 15.85 | 56663 | work 15.85 > 8.3 |
| grove | 3874 | 64.83 | 87.28 | 92.45 | 99.7 | 3874 | 13.89 | 8.22 | 16.43 | 69266 | work 16.43 > 8.3 |
| valley | 3851 | 61.65 | 70.47 | 88.12 | 106.5 | 3851 | 11.30 | 7.19 | 10.97 | 39534 | work 11.30 > 8.3 |
| coast | 3849 | 53.82 | 72.51 | 76.77 | 87.2 | 3849 | 8.64 | 6.53 | 11.00 | 9455 | work 11.00 > 8.3 |
| flight | 894 | 67.54 | 81.34 | 91.62 | 116.8 | 894 | 12.57 | 14.61 | 14.72 | 6459 | work 14.72 > 8.3 |
| walk | 5893 | 62.89 | 70.18 | 78.99 | 107.7 | 5893 | 11.45 | 8.92 | 15.59 | 40367 | work 15.59 > 8.3 |
| city-street | 3894 | 63.42 | 71.15 | 84.21 | 100.6 | 3894 | 11.86 | 11.50 | 16.84 | 40955 | work 16.84 > 8.3 |
| city-rooftop | 3897 | 62.76 | 70.20 | 79.33 | 97.9 | 3897 | 12.59 | 9.01 | 15.47 | 39830 | work 15.47 > 8.3 |
| city-approach | 3894 | 62.84 | 78.24 | 99.98 | 174.9 | 3894 | 9.96 | 9.47 | 18.42 | 51716 | work 18.42 > 8.3 |
| route-walk | 5891 | 65.12 | 72.47 | 77.44 | 95.6 | 5891 | 11.82 | 9.38 | 16.83 | 51538 | work 16.83 > 8.3 |
| city-bay | 2888 | 58.59 | 68.69 | 72.37 | 101.2 | 2888 | 8.59 | 8.49 | 15.22 | 24785 | work 15.22 > 8.3 |
| city-terraces | 2892 | 60.26 | 69.81 | 75.53 | 98.6 | 2892 | 10.38 | 9.55 | 16.70 | 43512 | work 16.70 > 8.3 |

## Phase 4 colour

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3865 | 62.10 | 69.97 | 74.49 | 91.8 | 3865 | 11.37 | 7.53 | 15.80 | 56669 | work 15.80 > 8.3 |
| grove | 3860 | 61.96 | 70.53 | 74.67 | 95.2 | 3860 | 11.20 | 7.77 | 15.87 | 69272 | work 15.87 > 8.3 |
| valley | 3847 | 61.50 | 68.33 | 72.08 | 113.0 | 3847 | 10.62 | 7.27 | 11.12 | 39540 | work 11.12 > 8.3 |
| coast | 3847 | 54.62 | 61.77 | 66.50 | 81.9 | 3847 | 7.86 | 6.38 | 10.49 | 9461 | work 10.49 > 8.3 |
| flight | 928 | 63.80 | 81.14 | 98.95 | 126.6 | 928 | 8.46 | 13.16 | 14.45 | 6560 | work 14.45 > 8.3 |
| walk | 5889 | 60.90 | 68.26 | 72.79 | 100.5 | 5889 | 9.47 | 8.57 | 15.11 | 40319 | work 15.11 > 8.3 |
| city-street | 3889 | 60.42 | 68.43 | 73.58 | 97.4 | 3889 | 9.50 | 11.07 | 16.39 | 40961 | work 16.39 > 8.3 |
| city-rooftop | 3888 | 59.89 | 68.30 | 72.58 | 88.5 | 3888 | 10.40 | 8.42 | 15.22 | 39836 | work 15.22 > 8.3 |
| city-approach | 3889 | 63.37 | 72.05 | 76.11 | 111.1 | 3889 | 11.91 | 8.62 | 17.55 | 51722 | work 17.55 > 8.3 |
| route-walk | 5887 | 64.53 | 71.27 | 75.23 | 97.8 | 5887 | 11.13 | 9.11 | 16.21 | 51702 | work 16.21 > 8.3 |
| city-bay | 2883 | 58.27 | 67.38 | 72.88 | 131.6 | 2883 | 9.66 | 8.11 | 14.76 | 24791 | work 14.76 > 8.3 |
| city-terraces | 2888 | 59.72 | 68.32 | 73.77 | 86.3 | 2888 | 10.03 | 8.84 | 15.87 | 43518 | work 15.87 > 8.3 |

## Phase 5 surfaces

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), 10 s warm-up, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p95 ms | Entities | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3862 | 60.47 | 69.13 | 75.01 | 86.9 | 3862 | 9.03 | 7.50 | 15.57 | 56670 | work 15.57 > 8.3 |
| grove | 3868 | 63.03 | 70.94 | 75.98 | 101.7 | 3868 | 11.40 | 7.90 | 16.42 | 69273 | work 16.42 > 8.3 |
| valley | 3848 | 62.30 | 69.31 | 72.45 | 81.6 | 3848 | 11.24 | 7.37 | 11.09 | 39541 | work 11.24 > 8.3 |
| coast | 3847 | 54.47 | 63.23 | 77.16 | 99.8 | 3847 | 8.04 | 6.54 | 10.48 | 9462 | work 10.48 > 8.3 |
| flight | 895 | 64.83 | 84.84 | 113.96 | 148.2 | 895 | 10.33 | 13.55 | 14.96 | 6476 | work 14.96 > 8.3 |
| walk | 5887 | 60.69 | 68.50 | 72.93 | 108.8 | 5887 | 9.21 | 8.81 | 15.42 | 40320 | work 15.42 > 8.3 |
| city-street | 3890 | 59.58 | 68.30 | 73.71 | 104.7 | 3890 | 8.56 | 11.14 | 16.85 | 40962 | work 16.85 > 8.3 |
| city-rooftop | 3889 | 58.24 | 67.46 | 72.75 | 101.5 | 3889 | 8.99 | 8.67 | 15.41 | 39837 | work 15.41 > 8.3 |
| city-approach | 3893 | 64.19 | 71.93 | 78.35 | 106.7 | 3893 | 9.31 | 8.98 | 18.52 | 51723 | work 18.52 > 8.3 |
| route-walk | 5895 | 65.14 | 72.27 | 77.20 | 169.4 | 5895 | 11.15 | 9.32 | 17.04 | 51703 | work 17.04 > 8.3 |
| city-bay | 2889 | 58.34 | 68.32 | 73.21 | 86.8 | 2889 | 9.60 | 8.48 | 15.15 | 24792 | work 15.15 > 8.3 |
| city-terraces | 2892 | 59.41 | 70.87 | 76.31 | 92.3 | 2892 | 9.58 | 9.55 | 16.53 | 43519 | work 16.53 > 8.3 |

## Phase 6 render thread

2026-10-04, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), static fixtures warmed until the foliage streamer is idle (10 to 90 s) then 3000 frames measured, moving fixtures 10 s, virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p50 ms | Render p95 ms | Entities | Foliage entities | Warm-up | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3000 | 49.46 | 51.22 | 59.51 | 65.9 | 3000 | 4.42 | 5.53 | 5.81 | 6.69 | 20902 | 16649 | 38 s settled | ok |
| grove | 3000 | 50.07 | 51.60 | 57.06 | 68.6 | 3000 | 4.59 | 5.73 | 6.31 | 7.22 | 24201 | 19853 | 39 s settled | ok |
| valley | 3000 | 49.72 | 53.08 | 59.74 | 69.3 | 3000 | 4.49 | 5.99 | 6.05 | 10.66 | 15126 | 10837 | 14 s settled | work 10.66 > 8.3 |
| coast | 3000 | 45.78 | 51.63 | 55.80 | 67.4 | 3000 | 2.85 | 5.47 | 4.03 | 10.58 | 7256 | 4888 | 14 s settled | work 10.58 > 8.3 |
| flight | 1066 | 54.69 | 65.18 | 72.08 | 120.6 | 1066 | 3.75 | 13.63 | 11.87 | 12.95 | 4744 | 1701 | 10 s fixed | work 13.63 > 8.3 |
| walk | 5875 | 48.78 | 55.46 | 60.41 | 76.1 | 5875 | 2.95 | 8.08 | 5.94 | 13.50 | 26683 | 10101 | 10 s fixed | work 13.50 > 8.3 |
| city-street | 3000 | 48.55 | 49.96 | 56.43 | 67.9 | 3000 | 2.91 | 8.31 | 6.44 | 7.16 | 27790 | 9399 | 41 s settled | work 8.31 > 8.3 |
| city-rooftop | 3000 | 49.34 | 50.87 | 58.44 | 62.4 | 3000 | 4.36 | 6.56 | 5.74 | 6.61 | 26026 | 8760 | 41 s settled | ok |
| city-approach | 3000 | 50.41 | 51.97 | 58.10 | 63.6 | 3000 | 4.62 | 6.38 | 6.63 | 7.56 | 27093 | 16165 | 42 s settled | ok |
| route-walk | 5874 | 49.86 | 56.62 | 61.58 | 75.0 | 5874 | 3.50 | 8.10 | 6.22 | 13.74 | 24248 | 13046 | 10 s fixed | work 13.74 > 8.3 |
| city-bay | 3000 | 49.00 | 50.70 | 59.06 | 64.2 | 3000 | 3.67 | 6.24 | 6.10 | 6.99 | 18717 | 6290 | 41 s settled | ok |
| city-terraces | 3000 | 48.80 | 50.53 | 58.84 | 63.8 | 3000 | 3.51 | 6.74 | 6.03 | 6.91 | 28406 | 10890 | 40 s settled | ok |

## Streaming

2026-10-04, NVIDIA GeForce RTX 4090, Linux, 2560x1440, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), static fixtures warmed until the foliage streamer is idle (10 to 90 s) then 3000 frames measured, moving fixtures held at their start until terrain and foliage are idle (same bounds), then measured from 5 s after they start (100 s for the walks, the flight to its end), virtual display (wall clock not meaningful).

| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p50 ms | Render p95 ms | Main max ms | Render max ms | Terrain uploads a frame | Moved m/s | Entities | Foliage entities | Warm-up | Gates |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| woodland | 3000 | 49.60 | 51.76 | 56.55 | 68.8 | 3000 | 4.42 | 4.99 | 6.00 | 6.85 | 8.2 | 9.9 | 0.00 |  | 20904 | 16649 | 35 s settled | ok |
| grove | 3000 | 50.13 | 52.56 | 56.73 | 66.8 | 3000 | 4.54 | 5.06 | 6.41 | 7.39 | 6.3 | 9.1 | 0.00 |  | 24203 | 19853 | 36 s settled | ok |
| valley | 3000 | 50.03 | 52.10 | 56.82 | 66.2 | 3000 | 4.66 | 5.05 | 6.24 | 7.17 | 6.7 | 10.4 | 0.00 |  | 15128 | 10837 | 35 s settled | ok |
| coast | 3000 | 46.00 | 48.19 | 53.61 | 64.2 | 3000 | 2.94 | 4.31 | 4.12 | 4.66 | 4.9 | 5.8 | 0.00 |  | 7258 | 4888 | 33 s settled | ok |
| flight | 1266 | 49.45 | 61.59 | 73.69 | 113.2 | 1266 | 4.58 | 7.34 | 5.44 | 6.64 | 25.7 | 23.5 | 5.01 | 200.03 | 4398 | 1677 | held 33 s settled | ok |
| walk | 1999 | 49.80 | 52.21 | 56.81 | 61.1 | 1999 | 4.03 | 6.39 | 6.60 | 7.60 | 8.9 | 10.3 | 0.11 | 4.99 | 25548 | 13781 | held 37 s settled | ok |
| city-street | 3000 | 48.86 | 51.30 | 56.39 | 63.5 | 3000 | 2.90 | 7.78 | 6.75 | 7.78 | 9.2 | 9.2 | 0.00 |  | 27794 | 9401 | 37 s settled | ok |
| city-rooftop | 3000 | 49.59 | 51.84 | 58.15 | 69.6 | 3000 | 4.50 | 5.90 | 5.93 | 6.82 | 7.2 | 8.5 | 0.00 |  | 26028 | 8760 | 37 s settled | ok |
| city-approach | 3000 | 50.51 | 52.66 | 57.07 | 65.8 | 3000 | 4.59 | 5.58 | 6.78 | 7.70 | 7.1 | 9.4 | 0.00 |  | 27095 | 16165 | 38 s settled | ok |
| route-walk | 2001 | 49.71 | 52.37 | 57.30 | 63.6 | 2001 | 4.16 | 6.47 | 6.40 | 7.61 | 10.1 | 9.6 | 0.14 | 9.00 | 24771 | 13946 | held 37 s settled | ok |
| city-bay | 3000 | 49.12 | 51.42 | 56.50 | 67.9 | 3000 | 3.70 | 5.71 | 6.31 | 7.25 | 6.6 | 8.9 | 0.00 |  | 18719 | 6290 | 37 s settled | ok |
| city-terraces | 3000 | 49.02 | 51.31 | 57.72 | 69.0 | 3000 | 3.51 | 6.20 | 6.34 | 7.29 | 7.4 | 8.5 | 0.00 |  | 28408 | 10890 | 36 s settled | ok |
