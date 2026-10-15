# Recovery after the 2026-09-13 freeze

All source changes survived the reboot. No task-owned game, capture or Cargo
processes remained when checked after recovery. Changes are uncommitted; the last
checkpoint remains `a353c17`.

Two GPU capture processes had already crashed before the full PC freeze. Previous
boot kernel logs identify the NVIDIA shader compiler, not a Rust stack overflow:

```text
12:45:43 Async Compute T: segfault ... sp 0000000000000000 ... libnvidia-gpucomp.so.595.84
12:50:48 Async Compute T: segfault ... sp 0000000000000000 ... libnvidia-gpucomp.so.595.84
```

This establishes where those process crashes occurred. It does not establish the
cause of the later whole-machine freeze. The inspected kernel records contained no
OOM-killer event or NVIDIA Xid explaining that freeze. Six-core affinity, sequential
windows and an 8 MB worker-stack setting had not eliminated the process crashes.
Do not treat the earlier flora-test stack workaround as a GPU fix.

## Completed

- Pure terrain fitter, shared Rust outpost content and shared PBR/mesh adapter.
- Three real seed-42 placements, generated foundations/access, matching collision,
  and startup vegetation reservations.
- Pure fitting tests, real-site reservation test, character traversal and rebase test.
- Full CI passed before the last grass correction; rustdoc and ordinary headless
  ten-frame smoke run passed.
- Captures exposed an existing grass reservation bug: `GrassBlade.x/z` are packed
  fractions, whereas plant coordinates are metres. The filter now uses `root()`.
  The focused `occupancy_decodes_packed_grass_roots_and_preserves_nearby_blades`
  regression passed before reboot.

## Remaining

1. Light CPU verification of the grass correction, then final full CI when the
   machine is stable. Use one Cargo job; do not combine builds with GPU runs.
2. Investigate the shader-compiler crash before resuming GPU verification. No
   driver or system configuration changes have been made.
3. Rebuild the game with the grass correction. Existing screenshots predate it.
4. Capture and inspect one view at a time, including all three sites and ground
   approaches. The script now prints a command by default, requires `--run` to
   launch one view, and keeps its log in this directory across reboot.
5. Repeat the standalone Rust recipe-preview smoke check after the extraction.
   The user retired the old building prototype, so its smoke check no longer applies.

There is no need to restart implementation or regenerate material assets. Do not
claim the old screenshots verify the final grass clearance.

## Resume investigation

Bevy 0.19.1's `RenderPlugin::synchronous_pipeline_compilation` bypasses its
asynchronous pipeline task spawning. The game and recipe preview now expose this
as `--serial-pipelines`. The first resumed slope overview completed with that
flag, four-core affinity and vsync, with no concurrent build. This is a tested
workaround for capture verification, not proof of a driver fix or the cause of the
whole-machine freeze. Normal rendering defaults remain unchanged outside the
opt-in study; the grounding study uses vsync to bound presentation work.

Resumed capture progress: `slope-overview.png`, `slope-ground.png` and
`uneven-overview.png` have been regenerated with the grass correction and inspected.
All three completed without a compiler crash using synchronous compilation. The
uneven ground-level view is next; both flat views still need regeneration. No
system or driver configuration was changed.

The uneven ground-level capture also completed and was inspected. Full CI passed
after the grass fix, serial-pipeline flag and removal of the retired building
prototype. The game and standalone preview rebuilt successfully. Flat captures
and the standalone Rust preview smoke check are the remaining integration checks.

## Integration verification completed

All six outpost captures were regenerated and inspected after the grass correction.
The Rust preview also rendered successfully. Full CI passed after cleanup with one
Cargo job and two test threads. No further compiler crash occurred during these
seven GPU runs with synchronous pipeline compilation and CPU limits. The original
whole-machine freeze remains unexplained; no driver/system changes were made.
The earlier Remaining list is historical and has been completed or superseded by
the user's removal of the old building prototype.
