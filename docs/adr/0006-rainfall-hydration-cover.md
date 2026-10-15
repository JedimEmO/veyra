# ADR 0006: Rainfall from the prevailing wind over relief; hydration and cover as sample fields

Status: accepted, 2026-09-12.

## Context

The climate stage's moisture is a stretched noise plus a coastal term. It makes deserts and forests exist but nothing about where they are is caused by anything: a forest sits where the noise is high. The vegetation of Phase 5 made that visible. Forests are uniform where they grow, and nothing in the world explains a dry side of a range, a wet valley or a clearing. The user's request: variation in coverage and hydration, simulated in the world generator, rain and where it falls.

## Options

1. **More noise.** A second, finer noise on moisture and a patchiness noise on tree density. Cheap, no cause, no rain shadows.
2. **A precomputed climate map.** Run a rainfall simulation over the whole world once and bake a grid the sampler interpolates. The best result and the classic approach, but a world of 100 km at one sample per 100 m is a million cells to compute and hold before the first sample, the sampler is no longer a pure function of the seed (it is a function of the seed and a bake), and every consumer (server, client, tools) has to carry or build the bake.
3. **Rain marched upwind per sample.** For each sample, walk a fixed number of taps upwind along a seed-chosen prevailing wind through a coarse continent field. Air picks up moisture over sea, loses it slowly over land and loses it quickly where the ground rises under it, so the windward side of a range rains and the lee is dry. The sample's rainfall is what is left plus what rains out on it. Allocation-free, stateless, deterministic, and the cost is a fixed number of cheap taps.

## Decision

Option 3, with two derived fields.

1. **Rainfall.** `WorldParams` gains a prevailing wind (a seed-chosen direction, as the ranges have a seed-chosen axis), a tap count and spacing, a land decay and an orographic rate. The climate stage marches the taps from the farthest upwind to the sample through a coarse continent evaluation (fewer octaves than the real one, since the walk only needs the land mask and the broad elevation), carrying moisture: full over sea, decaying over land, rained out where the elevation rises between taps. Moisture at the sample is the carried value plus the local rain-out, blended with a little of the old noise so that flat interiors still vary. This is what biomes classify on, so biomes move: a range now has a wet and a dry side, and the interior of a continent is dry unless the wind reaches it.
2. **Hydration.** A local wetness for the ground itself, in `Sample`: rainfall raised in hollows and lowered on slopes and ridges, from the relief of the sample's neighbourhood at a few hundred metres. Grass reads it for its lushness and density, the terrain tint reads it at range.
3. **Cover.** A fertility for trees and bushes, in `Sample`: hydration through a curve, times a patchiness noise of a few hundred metres, so a forest biome has clearings and groves and its edge is ragged. Placement multiplies a species' fit by it (trees strongly, grass weakly), and the terrain's far canopy multiplies its cover by it, so the clearings read from afar too.

## Consequences

- The world changes for every seed: biomes move with the rain. The seed 42 snapshots and the maps in `docs/research` are regenerated, and the plan records the new sample cost, which must stay under twice the old one or the tap count comes down.
- `Sample` grows two fields; every synthetic source in the tests sets them.
- The map tool writes moisture, hydration and cover maps next to the biome and height maps, because a climate is tuned by looking at it.
- The wind the foliage bends to and the wind that carries rain are not the same thing yet: the first is a client resource, the second a world parameter. Weather, when it comes, unifies them.

## Amendment 2026-09-26: erosion and the landform

Phase 7 of the [world slice plan](../WORLD_SLICE_PLAN.md) carves the relief the same way this ADR marched the rain: per sample, from the seed, with no bake.

- **Erosion.** `strata_worldgen::erosion` cuts gullies into the blended relief with stripes of directional noise, octave after octave, each following the slope the previous one left, so they branch. It is runevision's erosion filter (2026) reimplemented from its description; the gullies' spacing and seeds are one world field and only their strength is per biome (`Relief::erosion`), so gullies cross a biome border instead of jumping at it. It steers by the coarse relief's slope a couple of hundred metres out, the taps hydration already reads, because the fine relief's creases turned a gully by forty degrees in a metre and carved cliffs. Its trigonometry is polynomial and its kernel compact, so it keeps the correctly rounded rule.
- **The slope is the blend's.** Each biome's relief carries its analytic gradient from the noise kernel (the `noise` crate's own derivative is wrong, so `strata_worldgen::noise` differentiates the same value itself), so the slope no longer jumps where two biomes swap first place and costs no extra evaluations.
- **The landform.** `Sample` gains `landform`: slope, downhill aspect, the hollow term hydration already computed, and the finest gullies' ridge, from `-1` in a gully to `1` on a ridge. Drainage raises hydration in the gullies, and the terrain's far colour draws the ridges lighter and the gullies darker.

Every seed changes again, mostly in the mountains, highlands and forest hills, which are the biomes with erosion. A sample in eroded mountains costs 1.46 µs against the 1.5 µs the plan allows; lowland is 0.95 µs, up from 0.85 because the hydration taps are now blended over the biomes too.
