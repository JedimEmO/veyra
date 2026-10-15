# Equipment ownership fitting and supplies

Proposal, 2026-10-04. Exact draft item IDs and numeric values live in
[the catalogue](data/catalogue.json). All rules on this page describe intended
future behavior unless explicitly identified as current. See
[runtime gaps](runtime-and-validation.md) before implementing them.

## Weapon identity and fitting

A chassis defines a weapon's family, firing mechanism, primary hand position
and compatible slots. A stock configuration is complete and useful. Installing
a module replaces the stock assembly at one slot; it does not add another
copy of the same function. Removing the module restores the stock assembly
only if that physical part is available. Stock parts remain owned inventory,
not free items generated whenever a module is removed.

The draft represents stock assemblies implicitly to keep records small. A
future item migration must materialize them or retain them as tracked integral
parts with an explicit reversible conversion. It must not infer inventory
from the absence of a module. The instance example uses the latter model:
stock parts remain with that weapon, stowed while an accessory is installed,
cannot be traded separately, and cannot be duplicated by uninstalling it.

Every weapon requires a positive certification. If a character forgets a
required skill, the item remains owned but becomes unusable immediately.
Missing certification data is an error, never an unrestricted weapon. Parts
cannot grant the profession certification needed to use their chassis.
The proposed certification IDs are content concepts, not existing tree boxes.

Part changes require a workshop or gunsmith and an idle character out of
combat. The server validates the complete candidate build before committing.
Invalid builds leave weapon, parts, ammunition and currency unchanged. An
operation moves the actual installed item instance, and returns the removed
one. A part cannot be fitted to two weapons at once. Directly swapping between
already owned complete weapons can remain a field action with a swap time.

The three slot limit is a content target, not a universal rule: the blade and
baton only need two. Compatibility is an explicit allowlist by chassis and
interface revision. A part's silhouette, mass and attachment behavior are
authored. No continuous player scaling or arbitrary free placement.

## Ammunition and firing commitments

Plasma uses replaceable charged feed packs; conventional precision ammunition
uses cartridges and the scattergun uses pellet shells in a detachable box.
One plasma pulse or precision bullet consumes one ammunition unit. One shell
pays for the whole shotgun discharge, and that discharge's damage/posture
budget is divided over pellets. More pellets must not multiply its full damage
or stagger budget. No damage-type resistance table or automatic plasma
superiority is assumed.

A proposed three-pulse burst needs three available units to start. Spend each
unit as its pulse actually launches. A stagger or permitted dodge can interrupt
the unfinished burst; un-emitted ammunition remains in the weapon. The repeat
timer runs from accepted burst start and persists through interruption and
weapon swaps, so cancellation cannot accelerate first-pulse spam. Cancel any
remaining emission when holstering; do not fire a hidden weapon. Refuse a burst
with fewer than three units and show the reload need. Final absolute capacity
must hold at least one full burst. Exact intra-burst timing and cancel windows
remain profile/animation work, not inferred from a single-shot interval.

The support projector consumes feed units and gains heat per pulse. Holstering
retains heat and cannot reset its firing timers. Passive cooling may follow
played simulation time while holstered, using the same authoritative rule as
when held; it is not instant venting. Heat capacity, cooling delays and the
bracing transition require absolute profiles and simulation tests before tuning.

## Household and persistence

An individual weapon, installed module and implant have identities. Definitions
are shared; item instances are not. Household storage can be shared, but an
item equipped by a benched character is reserved to that character until
transferred at a valid place. A loadout preset requests items; it does not
duplicate them. The user-decided bench switching rules still apply.

Save the weapon's loaded ammunition, spare supply, heat and installed parts;
save actor pools and supply cooldowns independently of the loadout. Swapping,
reloading a save, installing a larger feed or moving between bench members
must not refill anything. Installing a smaller feed returns only the actual
excess rounds/charge to compatible storage, with the whole transaction refused
if there is no room. Heat follows its weapon through transfers.

Pool changes preserve current absolute amounts, capped at the new capacity.
Raising capacity adds empty space. Restoring a previous capacity does not heal.
The rule applies to learning/forgetting relevant skills as well as fitting.
The current game's full-pool-following behavior is not a suitable implementation
for freely swappable equipment.

## Implants

Propose four units of implant capacity per character, with each of these three
implants costing two. A character can therefore choose at most two. Each type
is unique on a character. Fitting/removal is a clinic service outside combat;
implants remain recoverable item instances. These are early planning limits,
not a full body-slot system or a new source of profession points.

| Implant | Benefit | Cost | Physical brief |
| --- | --- | --- | --- |
| Motor governor | Reduced ranged recoil recovery time | Slightly reduced sprint speed while installed | Flat, keyed neural interface cartridge with shielded contact edge |
| Tendon assist controller | Reduced melee attack stamina cost | Increased dodge stamina cost | Ribbed motor-control capsule with paired service contacts |
| Recovery regulator | Shield recharge begins sooner after damage | Reduced maximum stamina | Short ceramic-ended regulator with a recessed status window |

The governor is not an accuracy or damage multiplier. It will need an actual
recoil/recovery mechanic to matter. Tendon assistance does not change animation
speed, reach or contact windows. The regulator modifies the shield-specific
delay, not the current shared shield/posture hurt wait; splitting those timers
is required before it can be represented faithfully.

Housing artwork shows the implant in its handling tray or case. First-person
surgery, deforming skin, exposed body sockets and separate character rigs are
not prerequisites. External cuffs may later report implant state, but a cuff
is not itself the installed implant. Materials and keying distinguish these
small objects without relying on tiny printed text.

## Consumables

Propose three supplies. Each restores a fixed quantity on successful completion,
clamped to missing capacity. They provide no temporary maximum-pool bonuses.

| Supply | Paper effect | Use time | Physical brief |
| --- | --- | --- | --- |
| Field dressing | Restore 25 health | 3 seconds | Broad sterile pad in a flat peel-open rigid packet |
| Shield reserve cell | Restore 20 shield | 2.5 seconds | Short keyed cell with a wide insulated grip |
| Recovery ampoule | Restore 30 stamina | 2 seconds | Capped single-dose applicator inside a protective sleeve |

All three share a **45-second recovery-supply cooldown**, beginning at
successful completion. A character can carry at most six ready doses total,
with stacks up to five per type; other stock stays in storage. These limits
are proposals and must be visible in the UI.

One use at a time. The character must be alive, not staggered, and not committed
to another action. Starting use reserves one dose and cancels aiming; walking
is limited to 35 percent of ordinary walking speed. Shooting, attacking,
sprinting, dodging or a confirmed damaging hit cancels use. Cancellation
releases the reservation, consumes no dose, grants no restoration and starts
no cooldown. A full target pool refuses use before reservation. If the pool
becomes full before completion, release the reservation without consumption.
No partial restoration is granted while channeling.

At completion, decrement stock, restore the pool and start the cooldown as
one server transaction. The cooldown persists across weapon changes, bench
switching and save/load. A separate character has their own cooldown and
pools, but switching at a terminal cannot give the departing character free
recovery. A shield cell never refills plasma ammunition; compatible appearance
does not imply compatible electrical interfaces.

The shared cooldown makes a choice between health, shields and exertion. It
also prevents three consecutive consumables from erasing a mistake. Evaluate
those values in fights with enough duration for supplies to matter; today's
very short baseline duels cannot establish their value.

Consumables are optional recovery during an outing. Propose ordinary treatment
at a clinic and rest at the household home as reliable between-outing recovery,
with exact access, time and price still to design. A basic repeatable recovery
option must remain available to a household with no credits or supplies. This
does not imply passive combat health regeneration or free healing from bench
switching. Acceptance scenarios include stock melee and ranged builds with no
implants or consumables; supplies must not conceal an unviable base kit.

## Acquisition and workshop work

Basic complete weapons are purchasable or commissionable. A combat character
does not need to learn fabrication to stay equipped. Makers offer stock parts;
workshops supply alternatives and fit them. Exploration and contracts can
yield intact parts, repairable weapons and schematic knowledge. Routine
animals do not carry manufactured gun modules without a world explanation.

Use the same recipe through player and NPC fabrication, with service costs
added for commissioning. Exact recipes/prices await the resource model;
inventing a parallel ore economy here would conflict with small industry.
Relevant eventual inputs are certified electronics, conductive components,
structural metal/composite and purchased seals. Preserve lot and maker
provenance in finished goods.

The first DIY item is an adapted Morrow optic for Kestrel's burst-rifle mount.
It has an explicit adapter cost in bulk/handling, rather than universal access
to every other manufacturer's attachments. It occupies the sight slot as one
assembled module. The adapter is part of that module's recipe and geometry,
not another free stacking slot.

Do not introduce permanent durability loss for weapons in the initial proof.
Condition and repair economics can follow once the user has reviewed the
maintenance burden. Ammunition, supplies, commissions and optional workshop
services are enough candidate costs to evaluate first. No fixed buy/sell
numbers or guaranteed profitable crafting loop are asserted here.
