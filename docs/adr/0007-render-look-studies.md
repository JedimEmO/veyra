# ADR 0007: Exported render looks and fixed atmosphere studies

Status: accepted, 2026-09-12.

## Context

The daylight renderer needs a way to compare a darker, more atmospheric wilderness style without committing the game to one look. Local mist, sun shafts, occlusion and lens effects have different visual costs and performance costs. The dedicated server must not acquire render dependencies.

## Decision

`strata-atmosphere` owns a validated `LookFile`, containing sky settings and render effects, and applies it to its dressed cameras. Baseline preserves the existing ACES, EV100 14.2 and MSAA 4 rendering. Clear Contrast, Sunlit Mist and Cinematic Woodland are opt-in comparisons. The client owns an F6 slotted panel; the game owns CLI/file overrides. Export is explicit, separate from normal user settings. Normal startup reloads the exported `render-look.ron` beside the user settings file. An explicit look file takes precedence over an explicit preset, which takes precedence over the saved export. Missing exports use Baseline; invalid saved exports warn and use Baseline, while invalid explicit files fail. Further panel edits persist only when exported again.

Use Bevy 0.19.1 builtins for grading, AgX comparison, bloom, contact shadows, medium SSAO, volumetric fog, Gaussian depth of field and motion blur. The comparison presets use TAA because SSAO requires MSAA off. Explicit CLI AA wins; an MSAA override suppresses SSAO. Returning to Baseline removes the study's effects and required components while retaining pre-existing camera components, including water's depth prepass. Large transform jumps reset TAA history.

Local mist is one authored, textured volume at a selected seed-42 fixture. It has an `f64` WorldPosition and participates in the normal render-origin sync. It stays there when the camera moves away. A frozen visual sun changes rendering only; simulation time and network authority remain unchanged. This is not a streamed weather system.

## Consequences

The initial direction can be compared in the running world, with exact validated RON files for reproduction. The high-cost lens treatment stays in its own preset. Contact shadows remain an independent toggle, off in the presets after artifacts on thin vegetation. Mist density and scattered-light strength are independent because increasing extinction alone can darken the whole view.

GPU diagnostics bracket the camera driver, since summing Bevy's pass scopes misses fog and depth of field. The HUD prefers this whole interval and retains its old fallback when atmosphere is absent.

The [rendering study](../research/render-look-study.md) records captures, measurement conditions and limitations. Bevy's volumetric boundary behavior remains an upstream constraint; any failure should be isolated with a fixture and camera pose before considering a renderer fork. The default style and final production antialiasing choice remain open.
