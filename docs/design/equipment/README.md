# Weapons and field equipment design proposal

Updated 2026-10-05. This package contains the original combat and equipment
proposal plus the expanded [Rust weapon workbench](../../../tools/weapon-workbench/README.md).
The current authoring catalogue has 12 procedural chassis from four makers,
41 replacement components and 24 named configurations. Run
`just weapon-workbench` to assemble them. Live combat tables, profession trees
and replication registrations remain unchanged.

The [generated workbench catalogue](data/workbench-catalogue.ron) is the current
authoring data, emitted from the Rust definitions that drive the studio. The
older [JSON catalogue](data/catalogue.json), loadouts and combat briefs preserve
the original eight-chassis paper proposal, including three implants and three
consumables. They have not been expanded into combat definitions for Vigil.

Keep a weapon's handling identity through modification. Makers express distinct
construction styles, with controlled aftermarket adapters. Equal viability for
melee and ranged remains a design requirement, not an established balance result.

## Start here

1. [Weapons](weapons.md): the roster, combat jobs, weaknesses and exclusions.
2. [Manufacturers and art](manufacturers-and-art.md): construction language,
   procedural assembly constraints and briefs for the first modeling studies.
3. [Equipment rules](equipment-rules.md): fitting, ownership, implants,
   consumables and acquisition.
4. [Data contract](data-contract.md): draft records, stat semantics and worked
   configurations. [Catalogue](data/catalogue.json) and
   [loadouts](data/loadouts.json) preserve the original paper records.
   Use [workbench RON](data/workbench-catalogue.ron) for current authoring content.
5. [Runtime and validation](runtime-and-validation.md): existing capabilities,
   integration gaps, exploit cases and playtest requirements.
6. [Review](review.md): checks actually performed and unresolved decisions.

The local package validator is run with
`python3 docs/design/equipment/validate.py`. Its report checks data consistency
and prints relative configuration changes for the original JSON proposal. It
does not validate the expanded Rust catalogue or simulate combat.

## Established direction

These decisions came from the user in the planning conversation:

- Melee and ranged should be balanced as equally viable choices.
- A commando-like profession should support heavy weapons.
- Pistol, burst and sniper roles are attractive ranged distinctions.
- Plasma weapons belong in the setting. Shotguns and conventional projectiles
  are possibilities worth exploring.
- A few mainstream manufacturers should have recognizable styles, alongside
  adapted and DIY equipment. Avoid strange or exotic advanced technology now.
- Visible modification should preserve a recognizable chassis, use controlled
  compatible modules, and be practical to make with procedural ashlar geometry.
- The user subsequently requested an interactive Rust assembly UI with actual
  procedural meshes and materials. The workbench is that authoring tool.
- The latest direction adds a dark military line, more variations and components,
  and finer manufactured detail with moderate proportions. Avoid extreme bulk
  while retaining some stylization.

The existing [setting](../../SETTING.md),
[profession proposal](../progression.md) and
[small-industry proposal](../frontier-life/small-industry.md) remain context.
The household has a user-decided four-character cap and a 250-point budget per
character. This package does not replace those decisions.

## Current authoring catalogue

Names, assignments and the inclusion of individual models remain proposals.

| Maker | Models | Engineering emphasis |
| --- | --- | --- |
| Kestrel | Service pistol, burst rifle, support projector | Standardized institutional equipment with replaceable assemblies |
| Morrow | Precision rifle, field blade | Carefully controlled alignment, balance and handling |
| Bracken | Scattergun, heavy cutter, impact baton | Accessible mechanisms, repairable industrial construction |
| Vigil | Combat pistol, patrol carbine, marksman rifle, breacher | Dark coatings, lean receivers, recessed panels and restrained field fittings |

The precision rifle and scattergun are proposed conventional-projectile models;
the pistol, burst rifle and support projector use plasma. This is a first
product allocation, not a universal technology advantage or a complete
plasma/projectile matrix. Melee uses tangible blades and impact surfaces, with
industrial power assistance where appropriate.

The workbench includes optical accessories, feeds, grips, fore-ends, stocks,
muzzles, cooling, support and melee parts. Every exposed slot has a replacement;
two named configurations per chassis make the variation easy to compare. A
cross-maker optic adapter remains the first controlled DIY example. One housing
does not turn into every weapon class through a barrel swap.

Vigil's default patrol carbine opens with its maker filter selected. Both panels
scroll; the fitting bench offers presets, finishes and compatible components.
Stock reference compares geometry while preserving the fitted selection. Side,
top and perspective views support silhouette review. Fine coating, machined
metal and polymer graphs accompany portable mesh/material exports.

Regenerate current authoring data without rendering:

```sh
cargo run -p weapon-workbench -- \
  --catalogue docs/design/equipment/data/workbench-catalogue.ron
```

## Gameplay work after authoring review

| Stage | Deliverable | Exit condition |
| --- | --- | --- |
| Approve combat roles | Roster and role boundaries | Shotgun versus melee and commando versus rifle have distinct purposes |
| Approve visual grammar | Maker sheets and annotated concepts | Makers read through geometry; modules remain visibly compatible |
| Prove procedural assembly | Burst rifle with stock and two altered builds; field blade | Grips, clearances, material count, silhouette and export metadata pass inspection |
| Establish item authority | Definitions, instances, inventory and atomic fitting | Save/load, ownership, recertification and rejected operations conserve state |
| Integrate existing profiles | Pistol/carbine and a blade consume item definitions | Existing balance contracts still describe the same behavior before tuning |
| Add new mechanics | Burst timing, precision/spread, heat/bracing, melee differentiation | Each mechanic is covered by deterministic scenarios and played review |
| Add support items | Implants and supplies | Pool/cooldown persistence and stacking rules pass abuse scenarios |
| Expand content | Remaining approved chassis and modules | Reuse, performance and combat roles have been demonstrated on the small proof |

The procedural proof is now available in the workbench. The later authority,
animation and gameplay stages remain open; authoring exports do not establish
reload clearances or runtime item transactions.

## Decisions deliberately left for review

The draft chooses defaults so work can be evaluated without another planning
round. None of these are represented as user-approved:

- **Projectile roster:** approve conventional precision rifle and scattergun,
  or retain their roles while changing their delivery technology.
- **Melee breadth:** approve blade, cutter and baton before adding polearms,
  dual wielding, shields or unarmed combat.
- **Heavy weapon:** start with sustained, direct plasma fire. Launchers,
  explosions and area denial need separate encounter and networking work.
- **Fitting depth:** certification plus slot compatibility now. No temporary
  buff chains or overequipping rules in this proposal.
- **Commando progression:** define its skill-box route separately; do not
  silently introduce a new elite tree or spend the existing character budget.
- **Names and art:** maker names are working labels. Shape briefs require
  visual review before becoming production references.
- **Balance:** module factors are paper values. Absolute profiles for new
  weapons, prices, crafting times and quality progression await their models.

## Scope boundary

The files here are independent of terrain, rivers, sites and roads. JSON is a
review format with a dependency-free checker, not a proposal to replace the
workspace's RON registries. The data is intentionally outside `assets/` and is
not discovered by the game. Later implementation should use existing layering,
content tooling and the single simulation/balance model. `veyra-weapons` is
a pure content crate; `weapon-workbench` owns native meshing and rendering.
