# ADR 0001: Layered crates, one simulation, server and client as plugin groups

Status: accepted, 2026-09-11.

## Context

The target is an MMO-like game. Even the vertical slice, which is a single process, has to be shaped so a dedicated server later runs the same world and the same rules without a renderer. bevy-slotted already solved this shape for its domain: no-Bevy domain crates, Bevy adapters above them, a facade with `server()` and headless plugin groups, and a CI check that the server graph contains no UI or render crates.

## Decision

- The workspace is layered as in `docs/PLAN.md` section 3. Crates below `strata-sim` have no Bevy dependency. `strata-sim` and `strata-stream` depend only on Bevy crates with no renderer in them: `bevy_app`, `bevy_ecs`, `bevy_math`, `bevy_state`, `bevy_time`, `bevy_transform` and `bevy_tasks` (amended 2026-09-11 from "`bevy_ecs` and `bevy_app` only": states, the fixed clock, transform propagation and the compute pool are what a server runs too). Render-aware crates sit above them.
- There is one simulation, in `strata-sim`, running in a fixed-step schedule. The server runs it authoritatively. The client runs the same systems for prediction later and renders an interpolation of it.
- `StrataPlugins::headless()` and `StrataPlugins::client()` are the two entry points. `client()` is `headless()` plus rendering. `apps/game --headless` runs the first in the slice so the split is exercised from Phase 0.
- Authority is a port (`Authority` trait in `strata-sim`) with a `LocalAuthority` in the slice. Networked authority is an adapter in a later crate.
- Registries use `slotted-registry`. Game data (biomes, materials, later items, species, quests) is namespaced RON that loads, patches and freezes before the world exists.

## Consequences

- Terrain meshing cannot be in the same crate as region selection. The selection crate is testable headless; the meshing crate is not, and only the meshing crate needs `bevy_render`.
- `just server-check` is part of CI from Phase 0. It is cheap and it fails on the exact commit that leaks a render dependency downward.
- We take a git dependency on bevy-slotted pinned to a rev, patched to the local checkout in `.cargo/config.toml`. Bevy versions must match exactly, so a Bevy upgrade is a coordinated change across both repos.
