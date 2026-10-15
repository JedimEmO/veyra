# Equipment manufacturers and procedural art

Updated 2026-10-05. The original briefs below remain design proposals, including
their pose clearances and production budgets. A working
[Rust assembly studio](../../../tools/weapon-workbench/README.md) now realizes
12 chassis, 41 components and 24 configurations as procedural meshes and
materials. The [workbench catalogue](data/workbench-catalogue.ron) records current
authoring compatibility; the [original JSON](data/catalogue.json) preserves the
earlier combat proposal. Neither supplies live equipment integration yet.

## Current expansion: Vigil military equipment

Vigil supplies practical field equipment with dark charcoal coatings, muted
olive inserts and narrow machined receivers. Its construction language uses
recessed service covers, small selector marks, protected optics, thin stock
braces and vented front sleeves. Recognition should come from these shapes
before a logo or color. Keep the main masses simple and readable; smaller
fittings provide scale without exaggerated bulk or dense decorative greebles.

| Authoring chassis | Main visual distinction | Example component choices |
| --- | --- | --- |
| Combat pistol | Short faceted upper, slim grip and exposed trigger opening | Reflex sight, extended feed, angled or firm grip, vented or shrouded muzzle |
| Patrol carbine | Lean receiver, modular front sleeve and light rear stock | Prism optic, extended feed, guarded fore-end, open or supported stock |
| Marksman rifle | Long barrel, narrow spine and optic on separate mounting rings | Long optic, compact feed, field rest, supported stock and muzzle sleeve |
| Breacher | Short barrel group and broad lower support surface | Extended feed, reflex sight, open stock and alternate muzzle |

The military family shares compatible sights, grips, stocks and muzzle parts
only where the Rust catalogue permits. Feeds and fore-ends stay chassis-specific.
Two named builds per chassis establish useful starting silhouettes; they apply
ordinary components and carry no hidden bonuses. These models explore familiar
projectile construction without making a final combat technology allocation.

The same pass refines the original makers: smaller receiver bevels, recessed
fasteners, finer grip grooves, narrower stocks and scope clamps. The heavy
cutter now has a thin physical cutting head. Coating, machined-metal and polymer
graphs use weapon-scale grain and restrained normal detail. Issued, Field and
Bare remain finite material variants. The workbench bakes 512-pixel maps; Field
wear is a material study, not a gameplay durability state or an edge-aware wear
simulation.

The nearest-level meshes are authoring assets. Their separate material pieces
and fine boolean details still require draw-call, LOD and hand-animation review
before game integration. Refer to the [verification record](review.md) for
measured checks rather than treating the original budgets below as achieved.

## Original three maker briefs

Equipment belongs to Veyra's ordinary industrial economy. Plasma projectors
have a visible power cell, emitter and cooling surfaces. The proposed precision rifle and scattergun use conventional projectiles with
familiar cartridges, magazines and barrels. Neither needs gravity tricks,
living ammunition, dimensional storage or unexplained floating parts. A maker
can supply several institutions; branding does not imply allegiance.

| Working maker ID | Market and silhouette | Geometry grammar | Material grammar |
|---|---|---|---|
| `veyra:maker.kestrel` / Kestrel | Institutional supplier; compact, orderly service equipment | Chamfered rectangular receiver, short squared shroud, broad protected controls, enclosed cables; one repeated diagonal corner cut | Warm grey painted shell, charcoal polymer grip, dark metal heat surfaces; small muted amber inspection mark |
| `veyra:maker.morrow` / Morrow | Precision maker; deliberate, narrow profile | Long straight upper line, slim faceted casing, recessed fasteners, closely spaced parallel ribs; a distinct stepped front collar | Graphite coated metal, pale ceramic thermal insert, grey grip; restrained blue identification tab |
| `veyra:maker.bracken` / Bracken | Frontier industrial supplier; repairable, substantial tools | Broad split housings, stout cylindrical fittings, replaceable side plate, visible guarded service latch; no loose cable loops | Oxide brown paint, dull steel, black elastomer, localized primer patches; small cream stamped label |

Recognition must survive greyscale, rough lighting and a missing logo. Palette
supports the silhouette rather than replacing it. Smooth modest bevels catch
the existing lighting; economical geometry and worn materials follow
[SETTING.md](../../SETTING.md). Wear concentrates at contact edges, grips,
service latches and carried faces. Avoid uniform rust, noise over every surface
or a different custom texture set for every dropped item.

| Candidate weapon ID | Proposed launch maker | Visual role |
|---|---|---|
| `veyra:weapon.service_pistol` | Kestrel | Short service plasma sidearm with protected cell heel |
| `veyra:weapon.burst_rifle` | Kestrel | Common plasma rifle; the first modular ranged art proof |
| `veyra:weapon.precision_rifle` | Morrow | Conventional precision rifle with narrow spine and measured barrel collar |
| `veyra:weapon.scattergun` | Bracken | Broad conventional projectile receiver and blunt muzzle |
| `veyra:weapon.support_projector` | Kestrel | Plasma support tool with substantial heat jacket and larger power housing |
| `veyra:weapon.field_blade` | Morrow | Slim physical edge, service collar and replaceable grip panels |
| `veyra:weapon.heavy_cutter` | Bracken | Powered physical cutting tool with a serviceable power housing |
| `veyra:weapon.impact_baton` | Bracken | Rugged physical impact tool with guarded pommel |

These are primary assignments, not three complete copies of the roster. Morrow
does not need a pistol and Bracken does not need a precision rifle to fill a
catalogue. Damage, resources and firing rules belong to the equipment data
design, not to a finish or visual attachment.

## Chassis and finite modules

Each weapon has a fixed receiver, primary grip, support grip region, resource
well and silhouette family. The rifle's stock and shoulder contact stay fixed.
Its three functional slots are `sight`, `feed` and `fore_end`, each with a
stock module and compatible replacements defined in the catalogue. The first
replacements are `veyra:module.compact_optic`,
`veyra:module.rifle_extended_cell`, `veyra:module.rifle_cooled_fore_end` and
`veyra:module.adapted_morrow_optic`. The last is a narrow Morrow optic on a
workshop adapter shoe, supplied as one sight module rather than stacked parts. A fore-end includes
its shroud, cooling insert and muzzle frame as one module.

Allow all combinations of explicitly approved slot modules, subject to static
interface and whole-assembly clearance validation. Enumerate configurations
from the catalogue during the later geometry proof instead of fixing a count
here. Every largest combined optic, cooled fore-end and extended feed must
validate as an assembly; passing
three isolated module checks is insufficient. Begin the geometry proof with
three configurations: `rifle_stock`, `rifle_patrol` (compact optic), and
`rifle_sustained` (extended feed plus cooled fore-end), matching the geometric
parts of those paper loadouts. Implant effects are not geometry. The separate
`rifle_workshop` loadout introduces the adapted optic after that first proof.
Also inspect the stress fixture with all three
service replacements together. The adapted Morrow sight also needs a
whole-assembly clearance check, particularly with extended feed and cooled
fore-end. These proofs do not restrict the eventual legal roster
to three configurations. There is no arbitrary rail stacking, nested module
chain or cross-family fit.

DIY equipment reuses compatible service parts. A replaced grip panel, workshop
optic bracket or patched casing can show ownership without creating a new
receiver. DIY adapters consume the one slot they occupy and have their own
approved envelope. Tape, paint, repair plates and small maker marks are finish
variants or bounded detail; do not randomly move sockets or manufacture a unique
mesh for every item. Keep modifications mechanically understandable and preserve
the maker's recognizable receiver.

### Proposed attachment contract

Use local `f64` metres, right-handed Y-up, with +Z along the weapon toward its
muzzle and +X toward the user's left when held upright and aimed. This is a
right-handed frame: local +X crossed with +Y gives +Z. The origin is the centre of
the primary gripping region. No scale participates in attachment transforms.
Record an asset revision and an interface revision so a changed socket cannot
silently reinterpret an old assembly. These conventions are new content design,
not the current generated rifle's coordinate frame.

Each chassis/module record needs: stable part ID; interface family and revision;
socket frames; compatible chassis allowlist; occupied slot; required/forbidden
companion modules; local bounds; material keys; and clearance volumes. Match
family and revision as well as socket name. Matching dimensions alone grants
no compatibility. An invalid or missing module rejects the assembly rather than
silently substituting geometry.

| Frame or volume | Contract |
|---|---|
| `grip_primary` | Palm/grip reference; invariant within the chassis. Holds the whole weapon, including melee tools. |
| `grip_support` | Left palm reference plus a reserved gripping envelope; invariant for the first rifle's module choices. |
| `aim` | Direction reference along +Z. Front modules retain its centreline. |
| `muzzle` | Actual visible exit frame, supplied by the selected front module; +Z is exit direction. Changes length, never primary grip placement. |
| `mount_sight`, `mount_feed`, `mount_fore_end` | Physical interface frames. Socket local +Z points outward from the mating surface. Child `mate` uses opposed attachment unless the interface explicitly declares aligned frames. |
| `reload_resource` | Resource pack seating frame, insertion axis and withdrawal distance; separate from muzzle and gameplay firing origin. |
| `reload_clearance` | Swept pack volume plus hand access volume over the whole insertion/withdrawal path. No optic, grip, guard or DIY plate intersects it. |
| `hand_clearance`, `body_clearance` | Reserved hand, forearm, shoulder and torso regions checked in the reference poses. Clearance concerns the assembled item, not each module in isolation. |
| `blade_base`, `blade_tip`, `edge_plane` | Melee visual references for later animation/effects. They do not define authoritative hit detection. |

Grip frames should carry a documented palm orientation, not just a point. Keep
mount frames separate from grip references: a rail surface normal is not a hand
orientation. Compose the child transform from target frame, declared facing
rotation and inverse child mating frame. Never infer orientation from bounds.
Side-specific modules retain their handedness; mirror geometry only where the
authored fit and access volumes permit it.

## What ashlar supplies today

The local [recipe source](../../../../../bevy-crates/ashlar/crates/ashlar/src/recipe.rs)
provides parts, elements, material bindings and socket attachments. `attach`
opposes socket faces and `attach_aligned` coincides frames; building finalization
resolves the placements and rejects unknown sockets and cycles. This is recipe
assembly, not a live inventory attachment, bone-binding, module compatibility
validator or IK system. The proposed contracts above need game content metadata
and later integration. Do not claim that ashlar currently validates them.

Author solids with cuboids, chamfered cuboids, profiles, cylinders and boolean
operations. A cylinder starts on +Y and needs an explicit rotation for a barrel
along +Z. Keep bevel widths in metres and below the primitive's validation
limit. Union repeated ribs of one material within an element where useful;
unmerged elements remain separate pieces, so bolts should not each become a
rendered entity. Avoid coplanar detail overlays; use small scale-appropriate
offsets rather than copying a building's centimetre trim offset onto a gun.

Recipes can be evaluated offline with `ashlar-manifold`. The current
[prop format](../../../../../bevy-crates/ashlar/crates/ashlar/src/prop.rs) stores
mesh pieces, concrete material keys, levels, bounds, colliders and opaque numeric
tables. It has no typed equipment socket or animation contract. `Content::write_prop`
writes already generated meshes to `.ashprop`; it is not itself a recipe-to-prop
mesher. A future content generator must explicitly convert evaluated recipe
pieces into prop-local meshes and carry equipment frames in versioned game
metadata. A baked building is another assembly-capable output, but treating it
as a held weapon still requires game integration. Do not invent a ready-made
weapon exporter or GLB exporter.

Prebake the chassis and individual approved module meshes, then assemble them
in-game through future attachment integration. Whole-assembly bakes may serve
as preview fixtures, not mandatory production assets for every configuration.
Share reusable recipe parts and a small finish library. Recipe reuse alone does
not guarantee GPU mesh sharing or remove draw calls. Runtime assembly needs
module handle reuse, transform composition, visibility/lifecycle ownership,
combined bounds and muzzle metadata updates. Neither the current ashlar prop
loader nor the combat view supplies this complete equipment system today.
The geometry kernel stays in the content step; item compatibility and gameplay
data stay renderer-free under [ADR 0001](../../adr/0001-crate-layering-and-authority.md).

Bounds/hull collision is convex and does not preserve boolean recesses.
Held visuals need no automatic damage collider. Any later dropped-item collider
should be a coarse authored proxy; melee reach and hits remain simulation data.

### Production targets, not measured budgets

| Asset at nearest level | Triangle target | Visible mesh-piece target | Distinct material target |
|---|---:|---:|---:|
| Rifle, full approved assembly | 4,000 to 8,000 | 4 to 8 | At most 4 |
| Pistol or field blade | 1,000 to 3,000 | 2 to 4 | At most 3 |
| Support projector or heavy cutter | 5,000 to 10,000 | 4 to 8 | At most 4 |
| Implant cartridge or consumable prop | 200 to 1,000 | 1 to 3 | At most 2 |

Module triangles count inside the assembly total. Begin with 12 to 16 sides
for visible round fittings, fewer for concealed ones, and shallow physical
bevels on silhouette edges. Use material detail for grip grain and fine marks.
Share a small material family at modest resolution, initially up to 512 pixels
per baked map, with at most one 1024-pixel hero surface set if captures justify
it. Material graph variants are finite bake outputs, not unlimited free tinting.
The targets need mesh counts, actual rendered pieces, memory and engine captures
before acceptance. Do not apply building LOD thresholds unchanged to hand-sized
objects; test silhouettes and small gaps at combat camera distance first.

## First ranged brief: Kestrel burst rifle

Create an ordinary issued plasma rifle with an orderly chamfered receiver,
protected lower cell well and short square emitter shroud. It should read as
maintained service equipment that can be repaired in a frontier workshop. It
has no exposed glowing barrel core; a small status aperture is sufficient.
Stock, receiver and muzzle remain legible as three major masses in silhouette.

Nominal complete dimensions are 0.76 m long, 0.075 m wide and 0.24 m high,
including the inserted resource pack. These are blockout targets awaiting fit
against the existing aim pose. In the proposed frame, butt end is near Z=-0.22,
muzzle near Z=0.54, primary palm at the origin and support palm near
(0,-0.015,0.25). Keep a 0.10 m long uninterrupted support grip region. Set the
barrel centreline near Y=0.075; fit these offsets to the pose before freezing
the socket revision.

The resource pack seats below the receiver, forward of the trigger region, and
withdraws downward. Reserve a 0.10 m by 0.12 m cross-section for the pack/hand
path and 0.18 m of withdrawal travel beneath the well as initial clearance
targets. These volumes must be refined from the pack and reference hands, not
treated as measured anatomical guarantees. Keep the trigger/knuckle region open.

Offer a stock compact sight, service compact optic or adapted Morrow optic,
a standard versus extended
feed cassette, and a compact fore-end versus a cooled sleeve. None moves the
primary or support grip or changes the stock. The cooled front may extend to
Z=0.62; it provides its own muzzle frame. An optic cannot enter the cheek
envelope. The extended feed occupies additional space below the receiver and
must supply its own pack dimensions, extraction travel and combined clearance
check. The maximum combined optic/cooled/extended configuration must clear all
hands and reload paths. A different stock length, folding stock or relocated
well is a new pose/interface problem, not another harmless module.

Reusable recipe pieces: chamfered receiver, grip shell, resource pack, guarded
latch, optic base, shroud sleeve, heat insert and buttpad. Shell, grip, metal and
small indicator comprise four material roles. The DIY optic replaces the optic
module and uses the same mounting contract; one patched side panel is an
optional finish/configuration detail, not a fourth functional slot.

Later review views: side silhouette, top, front exit, underside reload path,
exploded slot arrangement, clay/wireframe, and existing body aim/fire/run poses.
Accept only combinations with clear pack withdrawal, intact hand volumes,
unobstructed muzzle, no clipping into chest/shoulder and recognition without
logos. No images or geometry are produced at this design stage.

## First melee brief: Morrow field blade

Create a slim single-edged physical blade with a visible service
collar and a clear guard, suitable for precise field work and close combat.
Target 0.72 m overall length including a 0.17 m grip, with a guard span of
approximately 0.07 m. Reserve roughly 0.53 m for blade and collar beyond the
grip/guard region; exact longitudinal allowances await blockout. The blade is
approximately 0.045 m wide with a 0.006 m spine, a clipped practical tip and a
plain cutting edge. These dimensions are pose-fit targets, not measured reach.
The blade axis follows +Z from the primary grip; document which face contains
the cutting edge. Avoid sawteeth, crystal edges, decorative light blades,
light trails built into the mesh and unsupported chains or tassels.

Use an extruded authored blade profile with a restrained bevel, recessed
service fitting, slim steel guard and two replaceable grip panels. Graphite
coated metal, a pale service collar and grey grip establish Morrow's identity.
The collar is a mechanical edge-service fitting, not a power requirement.
Wear sits on the edge, grip and guard, not every face. The edge slot has only
its stock assembly in this catalogue. `veyra:module.blade_balanced_grip` uses
inset grip weights and a restrained end-cap change while preserving the palm
frame, blade profile and approved hand envelope. Reinforced edges and extended
pommels are deferred possibilities, not current module options. Do not change
blade length without an explicit pose and clearance review. Heavy cutter and baton
silhouettes should not reuse this blade stretched to fit.

Record primary palm orientation, blade base/tip and edge plane. Later fit the
light/followup/heavy poses with the guard clear of fingers and blade clear of
the forearm. Do not equate a visually longer tip with increased combat reach.

## Implants and consumables

Use the catalogue's three implant forms: a flat keyed cartridge for the motor
governor, a ribbed capsule for tendon assistance, and a short ceramic-ended
regulator with recessed status window for shield recovery.
Installed implants need not become external body geometry. Identification comes
from keyed tabs, two or three restrained label panels and packaging; implants
are maintainable manufactured devices rather than glowing gems. No new clinic
brand is required yet.

Consumables use three reusable shape families: capped medical applicator,
rectangular power pack and flat peel-open rigid dressing packet. Vary label, cap and small
size within the family, while preserving the practical opening or insertion
direction. Do not assume that similar packaging makes resources interchangeable.
One shared case can package implants or consumables without another prop family.
Create hand-held versions only when an actual use animation needs them.

## Current animation boundary

[combat_art.rs](../../../crates/strata-client/src/combat_art.rs) loads the
generated `frontier_ranger.glb` body and eight indexed clips. It mounts the
generated `scrapyard_rifle.glb` at `RightHand` with a fixed quaternion and scale;
the legacy rifle points along local +Y. The pistol and glowing blade are built
from primitive meshes. Ranged visual muzzles are fixed children and the sim's
gameplay firing origin is separate. The view already adjusts poses for aiming,
reactions and the pistol's unsupported arm, but does not demonstrate arbitrary
module grip IK, articulated reload hardware or a dedicated pack-swap clip.

New content must first fit these reference poses through a future explicit
content-frame-to-hand adapter. Proposed metre dimensions must not inherit the
legacy prop's 0.9 scale silently. Grip, reload, muzzle and clearance metadata
document future needs; they do not promise animations already exist. Heavy tools,
different stock lengths, two-handed melee and unfamiliar reload motions remain
blocked on pose/art validation. The first art proof is the bounded rifle and
field blade, not all eight weapon families at once.

Longer front modules also need an authoritative obstruction contract. The
current sim uses its own shoulder-to-muzzle check, separate from the rendered
muzzle. A future renderer-free per-configuration muzzle/obstruction envelope
must agree with the fitted geometry, or every module must stay inside one
validated common envelope. Test a wall beside the muzzle and the end of cover:
neither a longer visual barrel nor a shifted effect may appear to fire through
an obstruction the weapon cannot clear. Likewise, publish agreed melee reach
metadata to simulation and verify the visual endpoints against it. Runtime
collision must never depend on reading a GPU mesh.
