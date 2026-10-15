# 0001: Bring the dense forest fixtures inside the frame gate

Status: open, 2026-09-27. Owner of the carried miss named in
`WORLD_SLICE_PLAN.md`'s Phase 12 record, item 1.

## Problem

The woodland and grove fixtures miss the frame gate (p95 at most 8.3 ms,
p99 at most 12 ms), and they have since the Phase 6 baseline.

| Fixture | p50 ms | p95 ms | p99 ms | GPU ms | Entities |
| --- | --- | --- | --- | --- | --- |
| woodland | 10.14 | 13.14 | 14.75 | 5.62 | 48 211 |
| grove | 9.67 | 12.33 | 14.09 | 5.80 | 68 761 |

The GPU is at 5.6 ms, well inside its budget. The frame is CPU-bound, and
the CPU work scales with foliage entities: every tree mesh level is two
entities (bark and leaves), every understory plant is two, and each is
culled, range-checked, extracted, binned and written into instance buffers
for the main view, the prepass and every shadow cascade it can reach.

## What we already know

- **Far trees are batches already.** An entity per far tree cost 60 ms of
  CPU at 12 000 draws. Moving them into one small mesh per cell and species,
  under one material per species, brought them to about 2 ms (Phase 9).
  That is the pattern to extend.
- **The entity count alone is not the whole cost.** Removing the city's
  30 000 out-of-range entities from the woodland moved p50 by 0.3 ms. An
  entity outside its `VisibilityRange` is cheap; one inside it, in several
  views, is not.
- **Phase 9's trace against Phase 8's** (same machine, same fixture, both
  still streaming; per frame, traced). The render thread grew 2.8 ms, led by:
  - `write_binned_instance_buffers<Shadow>`: +1.07 ms
  - `prepare_preprocess_bind_groups`: +0.58 ms
  - `unpack_bins`: +0.40 ms
  - `per_view_shadow_pass`: +0.26 ms

  Shadows are the biggest single share, and only level 0 and the understory
  cast.
- **The streamer prepares every level 128 m before its band** (`prefetch_m`,
  and the same again as `retention_m`). A tree's level 2 (to 385 m) is
  therefore installed for every tree within 513 m, and level 0 (to 66 m)
  within 194 m. Out of range they are cheap, as above, but not free.
- **Placement** is off the frame (about 21 ms a cell on a worker) and not
  part of this ticket; it is carried item 2.

## Plan

In order. Each step measures before the next, because each changes what
the next is worth.

1. **Measure the breakdown.** Split the woodland's entities and render-thread
   time by kind: tree level 0, 1 and 2, understory, grass and far batches.
   - Use `cargo run --release -p game --features trace` and aggregate the
     Chrome trace per system; the Phase 9 comparison script is the model.
   - Record the counts in the foliage log line, so the bench carries them.
2. **Shadow casters.** Count level 0 and understory entities in each
   cascade. Try:
   - understory without shadows beyond a short range (20 to 30 m), the
     way grass has none;
   - level 0 shadows only inside the first cascade.

   Either may change the look, so check them with captures, not only numbers.
3. **Prefetch.** Try `prefetch_m` and `retention_m` at 64 m against the
   flight fixture: fewer out-of-band entities, with the risk of late pop-in
   at 200 m/s.
4. **Level 2 as batches.** Draw each tree's coarsest mesh level the way far
   trees are drawn: one entity per cell, species, variant and piece, whose
   instance data rides in a small per-cell buffer or mesh, under one
   material per variant.
   - Wind at level 2 is the trunk's sway alone, which an instance can carry.
   - Level 2 casts no shadow, so only the main pass and the prepass need
     the path.
   - This is the step Phase 8 named, and likely the largest win: level 2
     covers 82% of the mesh band's area.
5. **Understory as batches.** The same pattern for ferns, flowers and
   bushes at their coarser level, which are the densest plants in a forest.
6. **Only then**, if still short: level 1 as batches, or moving the
   impostor handover nearer once atlases are block compressed (carried
   item 4). A 256 px frame makes a tree at 165 m sharp enough.

## Done when

- Woodland and grove hold p95 at most 8.3 ms and p99 at most 12 ms in
  `just bench`, with no frame over 33 ms after warm-up.
- Flight and route-walk hold their gates as well: no frame over 33 ms, and
  p95 no worse than at the Phase 12 exit (9.63 and 8.54 ms).
- No visible pop at a level switch in a still at woodland, and the trees
  still sway near and mid range. Checked by captures side by side.
- The ledger records the numbers before and after each step that landed.

## Out of scope

Placement cost (carried item 2), city piece streaming (item 3), and the
painterly shading.
