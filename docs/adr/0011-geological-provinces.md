# ADR 0011: Geological provinces, a field of their own beside the biomes

Status: proposed, 2026-09-30 (ticket 0004, step 3); for the owner to accept.

## Context

A biome is the climate's: what grows, and until now also what the ground is made of. Its `slope_material` was one grey rock nearly everywhere, and its relief kind was the only thing that shaped the ground. A wet forest on limestone and a wet forest on granite were the same place. Ticket 0004 wants ground worth looking at, and the research behind it found that most of what makes real country read as one place or another is its rock: which faces show, how its beds lie, how water cuts it, what lies about on it. The rock and the climate cross; neither follows from the other.

## Options

1. **More biomes.** A forest-on-granite and a forest-on-sandstone biome, and so on. Every climate window multiplied by every rock, the climate classifier choosing rock, and a rock border wherever the rain changes. Refused.
2. **Rock from the existing fields.** Pick the rock by elevation, range or a noise threshold. Cheap, but the rock then follows the climate's and the relief's own lines, and a noise threshold gives shares that swing with the seed.
3. **A registry of provinces and a field of their own.** A province is registry data like a biome, chosen per point by a field that has nothing to do with the climate, and the sample carries its pair and weights as it carries the biome pair.

## Decision

Option 3.

1. **The registry.** `assets/geology/*.geology.ron`, one file a province, loaded by `strata-registry` with the patch rounds the other registries have, at most sixteen. A province names its rock (the terrain material of its bare faces, a multiplier of the biome's bare slope, a far tint, a cover multiplier), how each biome relief class takes shape on it, its beds, how water cuts it, and its rock kit. Every section is validated at load even where no step reads it yet, so a wrong file fails today. Names are resolved at the freeze, which refuses a face past the sixteen layers the terrain draws and a frequency too small to be dealt a cell. Geology is optional: a set with none gives a world that is the world before provinces to the bit.
2. **The field.** `strata-worldgen`'s `ProvinceField`, per point and pure under the crate's float rule: jittered 24 km cells on a lattice turned by the seed, read at the continent stage's warped point so the coast's bends reach the borders; cells dealt provinces a block at a time by frequency, so every province appears in every block and none takes a land mass alone; borders bent by one octave of noise and blended over a kilometre. The order provinces are dealt in is data (`deal_order`), not the id's order, so adding or renaming one does not reshuffle the rest. Where a mountain range rises, a province may yield to the one its file names, so the ranges stand in the hard rocks and the plateaus stay plateaus. It costs about 60 ns of a sample's 1.75 µs on the mesher's grids, about 95 ns at scattered points.
3. **The sample.** `Sample::provinces` holds the two heaviest and their shares, as `biomes` does; `ProceduralWorld::provinces` gives the whole blend, up to every province, for the rare point where three or more weigh.
4. **Where biome and province both speak.** The biome keeps the climate, what grows, the relief class and the flat ground's material; settlement ground keeps its own. The province has the wild bare ground: its face wins over the biome's `slope_material` there, it bares at the biome's threshold times its `bare_slope`, its `far_tint` multiplies the biome's far tint, its `cover` multiplies the cover, and its kit's weight multiplies a rock species' biome fit. From ticket 0004's later steps it shapes each relief class the biome names, lays its beds and chooses how rivers cut it; the biome still says which class.
5. **On screen.** The terrain mesher packs the pair of faces into a vertex byte with a per-vertex triangle corner, and the fragment decodes all three corners' pairs, so a border draws two rocks in a ragged contact without a seam and without a new inter-stage location (the main pass has none to spare).

## Consequences

- Everything pinned that hashes the registries moves with a province file: `world_version` hashes every province, so the handshake refuses a client whose geology differs.
- The terrain's texture arrays carry a layer per province rock (five, 96 to 149 MiB of arrays), baked by `just content`.
- The sample carries only the top two provinces. Where three meet (under one percent of the land) the face and the rock kit switch pair along a line; the face spreads the switch over one triangle, placement does not. The whole mix is there for a consumer that needs it.
- Shares are exact only to a slot of the block the field deals (6 to 16 slots), which is why a frequency is bounded below.
- `just worldmap` writes a `-geology` layer, because a field like this is tuned by looking at it.
