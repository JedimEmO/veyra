# Corporate building study

See the subsequent [texture-scale correction](../building-uv/README.md) for the
updated material appearance. Images below preserve the initial study.

Second Rust-authored style, 2026-09-13. All images are actual Bevy renders of the
procedural geometry. The original frontier outpost remains available.

The study uses a heavy concrete podium, recessed glazed bays, projecting vertical
fins, a darker service spine, roof machinery, a sheltered lobby and sparse amber
accents. This explores a larger corporate presence without fixing the engine to
these proportions or materials.

| View | Purpose |
| --- | --- |
| [Tower](tower.png) | Four upper storeys over a tall podium |
| [Annex](annex.png) | One upper storey, using the same parts |
| [Courtyard](block.png) | Three heights and rotated wings assembled from one kit |
| [In-world](world.png) | Corporate tower fitted to the existing terrain and foliage |

## Authoring and reuse

`crates/veyra-buildings/src/corporate.rs` defines thirteen reusable parts, the
assemblies, material bindings, ground specification and static collision proxies.
`corporate::building(1..=8)` varies height by repeating curtain-wall bays and the
service riser. The courtyard composes three buildings without copying part
geometry. Selected bays override the glass slot with an occupied-window material.
The tower uses 12 parts across 70 instances (2,352 unique source triangles);
the courtyard uses 13 parts across 170 instances (2,396 unique source triangles).
These counts describe mesh reuse, not total drawn triangles or a performance benchmark.

`assets/buildings/corporate/materials.ron` is a separate palette. It reuses the
existing concrete and metal textures. Both styles use the same builders, Manifold
mesher, PBR upload adapter and terrain solver; no new geometry primitive was
necessary.

The courtyard is a standalone composition study. In-world placement currently
uses the tower at the three terrain test sites, selected with
`--grounding-style corporate`. Roads, district placement, interiors, transparent
glass and moving doors remain future work.

## Run and capture

```sh
CARGO_BUILD_JOBS=1 cargo build -p building-preview -p game
./target/debug/building-preview --serial-pipelines --scene corporate
./target/debug/building-preview --serial-pipelines --scene corporate-annex
./target/debug/building-preview --serial-pipelines --scene corporate-block
./target/debug/game --serial-pipelines --grounding-study flat --grounding-style corporate
python3 docs/research/building-corporate/capture.py tower --run
```

The capture script accepts `tower`, `annex`, `block` or `world`. Without `--run`
it only prints the command. It uses four CPU cores, serial shader compilation and
one GPU application at a time, following the earlier driver crash investigation.
World captures wait 420 frames for terrain and vegetation streaming.

## Verification

Full `just ci` passed after the final changes (format, Clippy, workspace tests
and isolated headless dependency checks). All four captures were inspected.

Both styles are meshed and checked for finite normal-map tangents. The game test
prepares all three actual corporate sites, including material resolution. The
headless character test walks the ramp to each style's closed entrance, then
checks building collision and visual transforms after a 5 km origin rebase.

Corporate footprint results for seed 42:

| Site | Terrain range, metres | Ramp length | Grade |
| --- | --- | --- | --- |
| Flat | 149.3760–149.4297 | 1.5 m | 0.2552 |
| Slope | 155.3335–157.1255 | 2.5 m | 0.2240 |
| Uneven | 157.5922–160.0783 | 6.0 m | 0.2730 |

These differ from the outpost because the corporate podium has a larger support
footprint. Terrain is unchanged; supports extend down to it and the access ramp
connects the raised landing to the sampled ground.
