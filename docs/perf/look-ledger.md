# Look ledger

GPU cost of the four look fixtures of `tools/look.py` (the lighting plan, `docs/research/lighting-fidelity.md`), appended by `just look --label "<text>"`. Compare two sections with `just look --compare <a> <b>`; numbers are for diffs within a session.

## Phase 0 baseline

2026-10-02, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-0-baseline-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 9.85 | 15.05 | 13.26 |
| dawn | 8.20 | 16.16 | 13.00 |
| dusk-street | 10.54 | 20.92 | 16.35 |
| coast | 8.79 | 12.47 | 12.27 |

## Phase 1 the sun

2026-10-02, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-1-the-sun-*.png.

The dusk-street fixture changed between these two sections: Phase 0 ran the live day at 0.73 from the player's orbit camera with the sun off screen; from here on it is a locked camera at 0.70 facing the sun (`tools/look.py`), so its delta against Phase 0 is a different frame, not a cost.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 9.93 | 15.59 | 13.30 |
| dawn | 8.58 | 16.05 | 12.69 |
| dusk-street | 9.85 | 15.25 | 13.71 |
| coast | 7.92 | 11.81 | 12.09 |

## Phase 2 the air

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-2-the-air-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 9.59 | 15.88 | 13.42 |
| dawn | 8.00 | 16.48 | 12.78 |
| dusk-street | 10.25 | 15.64 | 14.22 |
| coast | 7.20 | 12.20 | 12.40 |

## Phase 3 the shade

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-3-the-shade-*.png. The card held a low clock through these runs and the clouds were reshaped in the same phase, so the rise from Phase 2 is not the probes' (their in-run A/B is in docs/research/render-look-study.md). The coast row is from its bench json: that run was killed on exit by the machine's load crash after writing both files, and two clean reruns exited normally.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 11.04 | 16.83 | 14.52 |
| dawn | 9.14 | 17.26 | 13.59 |
| dusk-street | 10.23 | 16.27 | 15.04 |
| coast | 7.80 | 12.34 | 13.33 |

## Phase 4 colour

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-4-colour-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 6.82 | 16.34 | 12.87 |
| dawn | 5.24 | 16.88 | 12.57 |
| dusk-street | 6.40 | 15.92 | 13.19 |
| coast | 3.98 | 12.61 | 12.13 |

## Phase 5 surfaces

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-5-surfaces-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 9.35 | 17.42 | 14.03 |
| dawn | 7.16 | 17.21 | 13.04 |
| dusk-street | 9.01 | 17.11 | 14.70 |
| coast | 7.02 | 13.08 | 12.18 |

## Clouds v3

2026-10-03, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/clouds-v3-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 5.94 | 16.36 | 12.75 |
| dawn | 5.25 | 17.08 | 12.42 |
| dusk-street | 6.06 | 16.22 | 14.26 |
| coast | 3.69 | 12.96 | 12.11 |

## Phase 6 render thread

2026-10-04, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/phase-6-render-thread-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 12.86 | 14.58 | 13.10 |
| dawn | 13.49 | 15.82 | 13.29 |
| dusk-street | 12.21 | 15.29 | 14.11 |
| coast | 8.48 | 12.06 | 12.76 |

## Streaming

2026-10-04, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/streaming-*.png.

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 3.44 | 8.10 | 5.99 |
| dawn | 3.61 | 9.56 | 5.33 |
| dusk-street | 2.66 | 8.77 | 6.19 |
| coast | 2.03 | 5.29 | 4.69 |

## Pass 1 before

2026-10-10, NVIDIA GeForce RTX 4090, Linux, 1600x900, release, `baseline` look, virtual display (wall clock not meaningful), 400 frames, 5 s warm-up. Captures in scratch/look/pass-1-before-*.png.

Suspect: about twice the Streaming section's GPU means, taken minutes before an unattended driver upgrade (06:05) left the userspace and the kernel module apart. Not a baseline for pass 1; a rerun of HEAD in the same session as "Pass 1 fixed" supersedes it (`scratch/paint-fix/after.sh`).

| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |
| --- | --- | --- | --- |
| canopy | 6.00 | 6.54 | 4.13 |
| dawn | 6.63 | 7.44 | 4.25 |
| dusk-street | 7.53 | 7.34 | 5.41 |
| coast | 7.07 | 4.22 | 3.40 |
