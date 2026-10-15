# Draft equipment data contract

Proposal, 2026-10-04. The JSON files in [data](data/catalogue.json) are an
inspectable design dataset. They are not accepted by the game's asset loaders.
JSON lets the review checker use Python's standard library without adding a
Cargo dependency. A later implementation should translate the approved model
into the workspace's versioned RON registries and typed Rust definitions.

## Files and authority

| File | Purpose |
| --- | --- |
| [catalogue.json](data/catalogue.json) | Makers, property vocabulary, compatibility interfaces, certifications, chassis, modules, implants and supplies |
| [loadouts.json](data/loadouts.json) | Thirteen legal example configurations, their supplies and intended tradeoffs |
| [instance-example.json](data/instance-example.json) | Illustrative ownership and persistence shape, including one actual installed module and a damaged character |
| [validate.py](validate.py) | Executable consistency checks and a report of relative configuration factors |
| [test_validation.py](test_validation.py) | Rejection cases for malformed references, invalid combinations and broken ownership |

The catalogue is authoritative for exact proposed IDs, slot names, module
factors and supply numbers. Narrative documents explain intent. A contradiction
must be corrected rather than left as a second independent item definition.
All maker names and all individual item designs are proposals.

Every file declares `schema_version: 1` and
`status: "design_proposal_not_runtime"`. IDs are namespaced stable strings.
Names are display text, never lookup keys. Changing a display name does not
create a new item. A changed definition shape needs a schema migration; a
changed physical mating interface needs a new interface revision and explicit
compatibility review.

## Definitions

| Record | Required meaning |
| --- | --- |
| Maker | ID, working name and engineering identity. A maker is independent of institutional allegiance. |
| Metric | Scope (`weapon` or `actor`), benefit direction and unambiguous physical meaning. All current values are relative indices. |
| Interface | ID, revision, occupied slot, facing convention and pending geometry status. Revision 1 defines a proposed family, not measured connector dimensions. |
| Certification | ID and proposed progression route. `existing_box: null` explicitly means no current profession box grants this new permission. |
| Weapon | Maker, family, role, technology, purpose, nominal length, positive certification, slots and its own stock property set. |
| Module | Maker, one slot/interface/revision, explicit compatible-chassis list, property factors and a visible physical change. |
| Implant | Maker, capacity cost, unique-on-character rule, fitting location, weapon-family scope and actor/weapon factors. |
| Supply | Maker, pool and restoration amount, commitment time, shared cooldown, stack limit and physical form. |

Every slot records a proposed `stock_part`, an interface and a socket name.
Stock part identifiers describe integral stock assemblies, not separate
tradeable definitions. See the [stock policy](equipment-rules.md#weapon-identity-and-fitting).
Empty module lists mean a complete stock weapon, not a chassis with a missing
magazine or grip. Some slots intentionally have no alternative yet.

`absolute_profile: null` and `geometry_recipe: null` are deliberate blockers
to runtime promotion. A loader must not replace them with invented defaults.
Nominal lengths are blockout proposals, not physical measurements or authoritative
melee reach. `art_brief` is a relative documentation link, not an asset handle.
Acquisition entries describe sources, not implemented shops or recipes.

## Relative properties

Each stock weapon starts at `1.0` for every property it declares. Values only
compare configurations of that same weapon. A rifle's damage index of 1.0 and
a pistol's damage index of 1.0 do not mean they deal equal damage. No absolute
damage, heat capacity, firing rate, prices or crafting duration are invented
for the unimplemented families.

For an applicable property:

```text
configured_index = stock_index
                 * product(installed_module_factors)
                 * product(applicable_implant_factors)
```

A missing factor is identity, not zero. A module may only reference properties
declared by every compatible chassis. An implant's weapon factors apply only
to its declared family; actor factors always apply while installed, including
when its weapon benefit is currently irrelevant. The motor governor therefore
still costs sprint speed while holding a blade. Fitting a specialized implant
is a commitment, not an automatic best choice for every weapon.

There are no additive or Set effects in this draft. Each contributing instance
applies exactly once, in canonical ID order for eventual reproducibility.
Rebuilding starts from immutable base values, never from previously modified
results. This checker uses ordinary Python floats with a small comparison
tolerance; it is not a cross-platform deterministic simulation implementation.

Each individual factor and each composed equipment-only index must remain
within **0.65 to 1.35**. Reject illegal configurations; do not silently clamp
away a listed benefit or cost. Exhaustive checking includes legal implant
subsets, not just the hand-picked examples. These are provisional design bounds,
not future final combat-stat limits or evidence that profession bonuses fit.

The property vocabulary expresses intervals as time, not rates: `repeat_interval`
of 0.92 means an 8 percent shorter interval, equivalent to about 8.7 percent
more discharges per unit time before other limits. It does not mean 8 percent
more DPS after reloads, misses, bursts and heat. `recovery_time` is only the
post-contact melee commitment; it cannot scale a whole clip and invalidate
its measured contact window.

Capacity is a relative index in this design. After an absolute magazine size
exists, compose all allowed effects, floor to whole discharge units once and
require at least one, or at least three for a full three-pulse burst. Capacity
counts ammunition units, not bursts. The same future build report must show that rounding
actually changes capacity where advertised. Implant reductions to stamina
capacity are not rounded into free current stamina.

## Worked configurations

The exact inputs are in [loadouts.json](data/loadouts.json). The generated
[data review](data-review.md) prints all configurations; these illustrate how
to interpret it.

- **Rifle patrol:** compact optic makes aimed spread 0.90 and ready time 1.06.
  Motor governor makes recoil recovery 0.85 and actor sprint speed 0.95.
- **Rifle sustained:** extended cell makes capacity 1.25 and reload time 1.15.
  Cooled fore-end makes burst repeat interval 0.92 and aimed movement 0.92.
  The regulator shortens shield delay to 0.85 at a stamina capacity of 0.90.
- **Workshop rifle:** the adapted Morrow optic makes aimed spread 0.82, ready
  time 1.15 and aimed movement 0.95. It still fires the same burst pattern.
- **Blade runner:** grip and tendon assistance multiply melee attack stamina
  to `0.90 * 0.90 = 0.81`. Grip lowers posture per hit to 0.94, and the
  implant increases dodge stamina to 1.10. No faster animation is implied.
- **Commando lane:** jacket plus rest makes brace time
  `1.10 * 1.15 = 1.265`. Cooling rate is 1.15. Rest and governor make recoil
  recovery `0.90 * 0.85 = 0.765`, while governor still costs sprint speed.

The strongest legal combination is not necessarily one of these examples.
The checker enumerates every stock-or-module choice for every slot, rejects
duplicate slot occupation in examples, and includes all affordable distinct
implant subsets. A mechanically legal combination still needs geometry and
animation inspection before it becomes approved content.

## Individual items and state

`instance-example.json` shows a rifle equipped by the arrival and one adapted
optic installed in its sight slot. The parent references the module instance;
the module reciprocally references parent and slot. It cannot also be loose
stock or installed elsewhere. Two dressings are a stack with its own lot ID.
The arrival is damaged and has 23 seconds of supply cooldown remaining, making
the no-free-healing persistence requirement explicit.

The sample is **not a complete save schema**. It omits absolute capacities,
ownership migrations, full storage topology, loaded ammunition identities,
world position and learned boxes. Its rifle starts empty so it does not
invent a capacity for an unspecified profile. `material_lots: []` means this
example does not demonstrate production provenance, not that the eventual
crafting system may discard it.

The proposed sample stores remaining played-simulation seconds at save time.
The runtime may use an absolute simulation expiry and serialize remaining time
at a checkpoint. Either representation must survive reload and bench switching,
and the design does not authorize real-world overnight ticking in singleplayer.
Multiplayer uses server time and needs an explicit offline policy later.

## Promotion into the engine

Keep item definitions and compatibility plain data in the registry/domain
layer. Keep live ownership, actions and equipment effects in authoritative
simulation. Only the client resolves visual recipes, meshes and materials.
World-space drops use `WorldPos`; local socket geometry is weapon-local metres.
No ashlar mesher, GPU resource or native geometry kernel belongs in the server.

Use the existing skill effect semantics deliberately: first derive the selected
item's base rows, then apply approved profession effects, then equipment factors
once. Audit any skill Set effects against per-item rows before enabling this
order. Preserve skill unlocks and certifications separately from numeric effects.
The current registry field vocabulary cannot simply accept these JSON metric
names. New properties need types, units, validation and real simulation behavior.

Validate final absolute values after every source, including skills. Define
caps, rounding and action-window constraints in that shared implementation.
Do not create a separate Python combat model from these examples. Extend the
existing real-sim balance harness and use the same modifier path at spawn,
equipment changes, respec and load. Append protocol registrations in the
established order when actual networking integration is authorized.

Before a new weapon record can be promoted, it needs an approved absolute
profile, real certification mapping, geometry/interface metadata, animation
fit, persistence behavior and acquisition route. The approval gate is a
concrete implemented proof with measurements, not merely a valid JSON file.
