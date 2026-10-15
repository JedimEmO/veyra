# ADR 0002: `f64` world positions with a movable render origin

Status: accepted, 2026-09-11.

## Context

An `f32` has 24 bits of mantissa. At 10 km from the origin the spacing between representable values is about 1 mm; at 100 km it is about 8 mm, and a camera or a character animating at that distance visibly jitters. Bevy's `Transform` is `f32`. The world is meant to be tens of kilometres across in the slice and planetary later, and an MMO server holds all of it at once.

## Options

1. **`f32` everywhere and keep worlds small.** Rejected: contradicts the goal.
2. **`big_space`**: nested integer grid cells with `f32` local positions, an established Bevy crate. Strong, but it replaces `Transform` propagation with its own, touches every render-aware crate, and its grid model is more than a heightfield world needs today.
3. **`f64` in the simulation, one `RenderOrigin`, rebase when the camera moves far.** The sim and worldgen never see `f32`. Rendered entities get `Transform.translation = (world - origin) as f32`. When the camera is more than a threshold from the origin, the origin moves to the camera and every rendered entity's `Transform` is rewritten in one system.

## Decision

Option 3. `WorldPos(DVec3)` is the only position type in `strata-core`, `strata-worldgen`, `strata-sim` and `strata-stream`. `RenderOrigin` is a resource in `strata-sim` (it is a client concern, but the sim's `WorldPos -> Transform` sync system needs it and lives below the render crates). Rebase threshold: 2 km, so `f32` precision stays under 0.25 mm inside the rebased volume. Each observer (the camera in the slice, the local player later) reports a `WorldPos`; the stream crate works in world space and never in render space.

## Consequences

- Every entity that both renders and exists in the sim carries `WorldPos` and has its `Transform` derived, never the other way around. A `Transform` set by hand on such an entity is a bug, and a debug assertion catches it in `dev` builds.
- Terrain regions are rendered entities with a `WorldPos` at their corner; rebasing touches them like everything else. With a few thousand regions loaded the rebase pass is a single linear walk, and it happens at most every 2 km of travel.
- Physics later needs the same treatment; `avian3d` colliders live in render space and are rebased with their entities. That is a known cost and the reason the open question about `big_space` stays open.
- Server-side there was to be no render origin and no `Transform`, only `WorldPos`. Phase 4 amended that: the physics runs in `f32` render space, so a dedicated server keeps one `RenderOrigin` (it follows the first connected player) and every body carries a `Transform` for avian's sake; the server still never reads a `Transform` for game logic. ADR 0004 has the consequence for players far from that origin.
