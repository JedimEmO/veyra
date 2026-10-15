# Building terrain adaptation: implementation plan and handoff

Status: implemented and verified, 2026-09-13. See [delivery evidence and reproduction](research/building-grounding/README.md).
The sections below retain the original scope and implementation rationale.

## Outcome

Place the current Rust-authored outpost in the actual game world on three
repeatable sites: flat ground, a gentle slope, and uneven ground. Keep its floor
level, extend foundations into the terrain, provide a reachable entrance landing
and ramp, clear vegetation, and verify the result with the existing character.

The first delivery ends at a grounded, collision-correct building and reachable
threshold. A moving door follows in a separate delivery. Do not expand this into
procedural roads, terrain sculpting, material graphs, decals, settlement generation
or a general attachment editor; those ideas remain in `backlog/buildings.txt`.

## Context and constraints

- Rust types/builders and a replaceable geometry backend are the chosen direction.
  No macros yet. Keep style, architectural proportions and material choices in
  content; no mandatory concrete foundation type, bay grid or faction in core APIs.
- Preserve the engine's existing trees, terrain, lighting and player controller.
  Do not change world generation to make the fixture fit.
- The user retired the earlier building prototype during implementation. Rust
  recipes are now the sole building workflow; obsolete assets and tooling are removed.
- The prior implementation and screenshots are already present. They were
  uncommitted when this plan was written; the user subsequently requested a
  checkpoint commit before the fresh session. Inspect current status and local
  instructions; preserve any newer uncommitted work and unrelated rendering changes.
- User authorization includes implementing this plan. Begin work in the fresh
  session without another plan-approval question. Do not spawn subagents unless
  the user or applicable instructions explicitly request delegation.

## Current code to inspect

| Area | Entry points and current limitations |
|---|---|
| Rules and layering | `CLAUDE.md`, `docs/PLAN.md`, ADRs 0001, 0002, 0003, 0004, 0008 |
| Recipe domain | the `ashlar` crate in the sibling ashlar workspace: local f64 Y-up geometry, parts, instances, sockets, material libraries; no world or renderer dependencies |
| Mesh backend | `ashlar-manifold`: Manifold adapter, shared element meshes, cut-face material batches, and the collision proxies elements declare |
| Outpost content | `crates/veyra-buildings/src/lib.rs`: shared facade/entrance, roof and a thin fixed foundation slab; current door panels are static |
| PBR preview | `tools/building-preview/src/main.rs` over `ashlar-preview`, with `ashlar-bevy` for texture loading, tangents, mesh sharing and deterministic captures |
| Material assets | `assets/buildings/recipe-study/materials.ron`, textures; do not regenerate these during placement work |
| Terrain collision | `crates/strata-physics/src/terrain.rs`, terrain mesh code, `crates/strata-flora/src/lib.rs` ground interpolation; agree on the actual LOD0 triangle surface |
| Vegetation clearance | `crates/strata-sim/src/footprints.rs`: shared immutable world rectangles consumed by foliage and tree collision, installed before streaming |
| Visual baseline | `docs/research/building-materials/README.md` and captures; `docs/BUILDING_RECIPES.md` for the current workflow |

Read those relevant slices, rather than reconstructing the previous conversation.
Verify Bevy/Avian APIs against the installed crate sources.

## Implementation sequence

### 1. Describe the building's relationship to the ground

Add a small, renderer-free grounding description next to the recipe domain:

- Explicit local support footprint, distinct from the overall visual bounds.
- Authored floor reference and underside of the retained floor/foundation slab.
- Named entrance frame, clear width, threshold elevation, and outward approach.
  Reuse the existing `Socket`/`Pose` vocabulary; do not infer entrances from names,
  mesh bounds or screen-facing axes. The current facade faces local negative Z.
- Configurable limits: clearance, embed depth, support span/depth, ramp width,
  maximum grade and available approach length. Choose and document study values;
  these are content/configuration, not global engine rules.

Preserve the upper outpost geometry. The existing thin slab can remain as the
floor/cap, with site-specific support generated beneath it. Ensure the declared
floor/threshold match that actual geometry rather than assuming every part starts
at Y=0. Add explicit static collision proxies for the study's load-bearing pieces;
decorative trim and tiny details should not dictate player collision.

Share the recipe content between preview and game without making the game depend
on a binary tool or putting study-specific parts into the core crate. Prefer a
small renderer-free content module/library if direct Rust reuse is straightforward;
a documented generated RON fixture is also acceptable. Do not maintain hand-edited
duplicate definitions. Extract reusable PBR/mesh upload code into an appropriate
render-side adapter only as needed by the two consumers.

### 2. Implement a pure terrain-fitting calculation

Inputs: a validated grounding description, requested world X/Z and yaw, a sampled
terrain patch, and configurable placement limits. Keep calculations in f64.
Outputs: a placement result containing level floor elevation, foundation/support
segments, entrance landing/ramp geometry descriptions, occupancy footprints and
useful diagnostics. Return typed failure reasons for unsuitable sites.

Keep world sampling outside the recipe domain. The game gathers data through its
existing world source; the solver consumes plain samples and can use analytic
fixtures in tests. Match the collision/rendered ground's LOD0 triangle interpolation,
including cell boundaries. Do not rely solely on an arbitrary coarse height grid:
include relevant terrain vertices/intersections under the footprint and approach,
or state and test a conservative bound covering unsampled extrema.

Start with vertical foundations beneath the authored slab perimeter, split at a
bounded spacing. Each segment extends below the minimum terrain over its span by
an embed margin; its top meets the slab underside. Inspect the full span, not just
its endpoints. This permits overlap below ground without terrain editing. Reject
excessive relief or support depth instead of silently making enormous stilts.

Keep the slab above the highest ground under the occupied footprint. A maximum
height plus configurable clearance is a suitable first policy, subject to approach
constraints. Calculate a short landing and a straight ramp from the entrance's
outward frame, sampling and checking the whole corridor and the ramp's ground end.
Bound the search for a reachable endpoint and maximum grade. Prefer a ramp for the
first player test; stairs can be a later access policy. Reject a site if no valid
approach fits the configured limits. No road network or switchback planner here.

The output must also be the source for collision proxies and vegetation clearance,
so visible supports, traversable access and occupancy cannot drift independently.

### 3. Integrate an opt-in game-world fixture

Add a separate opt-in Rust building grounding study, for example
`--building-grounding-study`, following the existing game CLI conventions. Keep it
mutually exclusive with incompatible old-study/network/benchmark modes. Do not
change ordinary world startup or silently load Manifold assets into the server.

Select and record three dry seed-42 sites after sampling actual terrain. Record
coordinates, yaw, measured relief and resolved elevations. Do not guess coordinates
or label a site as sloped merely by appearance. Include at least one non-axis-aligned
placement. Render the same outpost content at every site with site-specific supports.

Compute fitting and boolean meshes before interactive startup or on a bounded
worker task. Do not evaluate CSG in a per-frame system. Reuse upper-building meshes
and material handles across sites; only foundations/access vary. Use stable
`WorldPosition` roots and the existing `RenderOrigin` rebasing mechanism. The
standalone preview's local recentering is not a substitute for world coordinates.

Install foundation/building/access static colliders from explicit descriptions,
with the same poses and dimensions as their visible geometry. Preserve the doorway
opening; static closed door leaves may still block entry at this milestone. The
character must be able to reach the landing and threshold without snagging.

Compute clearance before foliage/tree-collider jobs are scheduled. Extend the
existing `VegetationExclusions` resource with building, support and approach bounds
rather than replacing other exclusions. Conservative rectangles are sufficient
initially, with a modest configured margin. Avoid a new dynamic vegetation
invalidation system: these are startup fixtures.

### 4. Verify in tests and in the game

Meaningful pure tests:

- Flat plane: stable floor, minimal supports, correct threshold connection.
- Sloped plane: level upper building, different support depths, grade-limited ramp.
- Uneven patch: interior high point clears the slab; a depression beneath a support
  span does not leave a visible gap; invalid/unsuitable inputs fail explicitly.
- Arbitrary yaw and translated world coordinates preserve fitting relationships.
- Changing material bindings does not change the placement result.

Integration checks:

- Actual character collision can ascend the approach and reach the threshold;
  walls/supports remain solid, ramp ends have no unexpected step or penetration.
- Rebasing leaves visuals, collision and world-space occupancy aligned.
- Foliage and tree colliders both respect the complete footprint/approach; nearby
  vegetation remains present.
- Generate reproducible overview and ground-level captures at all three sites,
  plus a collision/wireframe view where useful. Inspect foundation contact and the
  ramp's complete route, including the downhill side. Document sampled metrics.
- Ordinary startup and the Rust recipe preview still work. Ensure the
  headless dependency graph stays free of renderer/native mesher dependencies.

Store capture commands, images, site data and results under
`docs/research/building-grounding/`. Add a short workflow section to the recipe docs
and update the main plan with the delivered scope and remaining limitations.

## Completion criteria

The same Rust outpost is visibly and physically grounded at three documented sites
in the existing game world. Its floor stays level, terrain does not intrude above
it, supports reach the ground across their spans, and the player can reach its
entrance by the generated approach. Vegetation clearance matches both rendering
and physics. Excessively difficult sites fail with a useful reason. Geometry and
placement policies remain style-independent, and automated checks pass.

No moving door, road generator, global terrain changes, production asset exporter
or new networking protocol is required to call this delivery complete.

## Operational handoff

Read current local rules and status first. The PC previously froze under excessive
parallel load: use one Cargo job, modest worker counts, and sequential GPU runs.
After recovery, captures use `--serial-pipelines`, four-core affinity and vsync.
Manifold's first native build may download pinned C++ sources and invoke parallel
CMake; compiled dependencies already exist locally, so avoid cleaning the target.
Use CPU affinity if a clean native build becomes necessary.

Previous material delivery passed eight new tests, rustdoc and full CI. Full CI
uses the existing workaround for unrelated flora test stack instability:

```sh
CARGO_BUILD_JOBS=1 RUST_MIN_STACK=8388608 RUST_TEST_THREADS=2 nice -n 15 just --tempdir /tmp ci
```

Run appropriate focused tests first, then `just ci` before finishing implementation.
GPU windows may need sandbox escalation. Screenshot mode should ignore live
view/palette input so the user can keep working during captures. No processes from the original planning task need to be resumed.

Suggested fresh-session instruction:

> Implement docs/BUILDING_TERRAIN_PLAN.md. The plan is already approved; begin with
> the grounding description and pure fitting solver, then complete the in-world
> three-site study and verification. Preserve existing uncommitted work.
