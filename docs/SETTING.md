# Veyra: a world still being made

Setting direction, 2026-09-13. Names are working names. This document defines
the world and the intended player experience; [PLAN.md](PLAN.md) tracks the
engine's implementation and what currently runs.

## Premise

**Veyra is an inhabited frontier planet undergoing centuries of terraforming.
You arrive to build a life on a world whose future is still being negotiated.**

Passenger shuttles arrive regularly. Cities have universities, implant clinics,
nightclubs, housing shortages, and competing employers. Beyond them lie
inhabited valleys, isolated industrial towns, alien wilderness, and regions
where settlement is only beginning. The wider civilization remains connected
to Veyra through migration, trade, and institutional interests.

Different generations built different versions of the planet. Early settlers
engineered forests and agricultural plains. Later projects created warmer
coastlines, diverted rivers, and introduced organisms adapted to local
conditions. Their systems overlap, sometimes uneasily. Native life keeps
finding unexpected ways to adapt.

Terraforming continues because people want different things from Veyra: more
farmland, preserved ecosystems, new cities, profitable extraction, or somewhere
beyond institutional control. Mature regions can be comfortable and reliable
while the next valley is still being transformed.

The central question is **who gets to shape a place that other people already
call home?**

## Atmosphere and everyday life

The [screenshot studies](research/veyra-style/README.md) explore a coastal city,
an atmospheric processor, and a woodland settlement based on the current game
landscape. The [latest woodland reference](research/veyra-style/gritty-v4.md)
tests grittier, moderately detailed assets within the existing environment.
The [waterfront, service yard, and freight-stop studies](research/veyra-style/scenes-v5.md)
extend that direction to other places in the region.
The [corporate concrete studies](research/veyra-style/corporate-v6.md) add
Pelagic districts and facilities with a darker, more imposing architectural
identity.

Keep the engine's current terrain, relatively high-fidelity trees, foliage,
water, and lighting. New assets use economical geometry, modest smooth bevels,
convincing materials, and restrained surface detail to fit that environment.
The workflow combines local Trellis2/Pixal3D generation for assets with
Rust-authored recipes for modular buildings. Strong silhouettes and reusable building parts
guide the artwork; actual asset budgets need validation in the engine. The
fully faceted low-poly experiment was rejected.

The built world should show age, work, and uneven investment. Mix construction
generations, replacement panels, painted-over markings, practical additions,
and localized wear. Maintained services and personal touches give worn places
an ongoing life. Frontier buildings should have individual histories rather
than all sharing a pristine, coordinated finish.

The tone is strange but tangible, with beauty, danger, and ordinary life sharing
the same landscape. Anarchy Online is the main atmospheric inspiration:
distinctive futuristic cities, quiet journeys, strange wildlife, and a world
worth inhabiting. Veyra's own identity comes from its competing visions of
planetary development.

Corporate city districts favor massive dark concrete, deep recessed entrances,
narrow windows, enclosed transit connections, and controlled approaches.
Repetitive housing and administration blocks express institutional scale and
authority. Cold work lights and sparse markings sit within the structures'
shadow. Other districts can contain lighter composites, improvised commercial
streets, and individually modified buildings. Infrastructure creates recognizable
skylines. An implant clinic might occupy the floor above a crowded transit
concourse, with freight skimmers passing its windows.

Frontier settlements have prefabricated buildings expanded over generations,
workshops beneath immense machinery, improvised markets, and surprisingly
comfortable homes. People care about reliable services, their neighbors, and
the opportunities that brought them here. A distant settlement should have a
reason to exist and people with reasons to stay.

Wilderness has recognizable terrain and unfamiliar ecology. Imported trees
shelter native grazers. Engineered plants follow buried water infrastructure.
Abandoned cultivation projects have become distinctive habitats. Moving away
from maintained routes gradually exposes the traveler to less understood
ecosystems and less dependable support.

Implants, engineered bodies, drones, medical fabrication, and civilian aircraft
are familiar parts of society. Equipment needs power, materials, maintenance,
and expertise. Spectacular technology has physical presence and practical
consequences that people can understand.

An hour outside a city, a traveler crosses wet grass beneath an immense
atmospheric processor, its warning lights visible through low cloud. There is
a settlement under its service platforms. Someone there knows why the
surrounding forest has started changing.

## Institutions and competing interests

These three groups recur across the opening region. Each contains competing
personalities and agendas. Cooperation with one does not automatically make
every other group an enemy; affiliations develop through play.

| Group | What they offer | What they want | A source of friction |
|---|---|---|---|
| Pelagic Works | Reliable infrastructure, employment, advanced equipment | Long concessions and authority over development | Essential maintenance gives the company influence over communities that depend on it. |
| The Borough Assembly | Local government, mutual aid, independent enterprise | Settlements controlling their land and essential services | Members disagree over how to share water, costs, and the consequences of expansion. |
| The Continuity Institute | Ecological knowledge, medicine, environmental monitoring | Limits on changes that could destabilize the planet | Restricting development can protect a region while denying a settlement needed opportunities. |

These interests create work for explorers, engineers, traders, and mercenaries
in the same places. A dispute can involve employment, personal relationships,
technical evidence, and local needs as well as institutional allegiance.

## Arrival in the Serein Basin

The player is a new arrival through **Port Serein**, a coastal city accepting
settlers and skilled workers. Their background explains their existing skills;
their local reputation and affiliations emerge from what they do here.

The opening region is the **Serein Basin**: the port, agricultural lowlands,
wooded uplands, an inland service town, and an unfinished expansion zone. An
enormous atmospheric processor is visible from much of the basin. Its silhouette
helps travelers orient themselves and connects the region's everyday life to
the machinery sustaining it.

The basin provides a gradual journey from established urban life to frontier
uncertainty. Roads and service routes connect destinations; trails and visible
landmarks invite detours. Each destination has work, inhabitants, and a role in
the surrounding region.

An early job concerns a failed monitoring station. A paid repair can uncover
missing equipment, a nearby settlement's unauthorized water diversion, and an
ecological change somebody would prefer to keep quiet. This is a narrative
seed for related work and relationships. Exploration, technical work, trade,
dialogue, and combat can grow from those circumstances.

Missions reference actual places, people, and equipment. Returning to a
settlement should allow players to encounter familiar residents and recognize
the consequences of their work.

## Engine direction and first playable proof

The next design priority is turning the existing landscape into an inhabited
place. These capabilities are future work, ordered by what the journey needs:

1. **Settlements and landmarks.** Place authored locations within procedural
   terrain, with stable identities, approaches, and vegetation clearings.
2. **Connected destinations.** Add roads, trails, service routes, and readable
   entrances. Buildings and interiors follow from the needs of those places.
3. **People and interactions.** Add residents, service providers, affiliation,
   dialogue, and usable machinery.
4. **Missions and persistence.** Reference real residents and equipment in
   objectives. Completed work can leave a saved, server-owned local consequence.

The first playable proof is a journey from a small port district to a service
settlement and a monitoring station. It should establish the following:

- The route is recognizable through its landscape, connections, and landmarks.
  The atmospheric processor remains a useful point of orientation.
- A player can meet a resident, accept a job, travel to the station, operate
  equipment, and return for a response.
- Revisiting the station preserves the resulting equipment state, including
  after unloading and reloading the location and after a server restart.
- Two connected players see the same shared equipment state. A player arriving
  after the interaction also sees its resulting state.

World placement must continue to agree between server and client. Persistent
local changes build on the seeded world, following the existing architecture's
provision for procedural content plus persisted edits. Veyra-specific names,
institutions, and missions belong in game content over reusable engine systems.

The initial setting covers one planetary region. Terraforming provides its
history, geography, and local problems. Large environmental transformations
remain a later design topic. Civilian aircraft and busy city life establish
the setting's identity; their full simulation is beyond the first journey.
