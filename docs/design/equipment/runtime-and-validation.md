# Equipment runtime boundary and validation

Design proposal, 2026-10-04. This package changes design and proposal data only.
Its indices compare configurations of the same stock chassis. They do not
compare chassis damage, predict time to kill, or establish melee/ranged parity.
Module factors between 0.65 and 1.35 are bounded paper inputs, not permission
to change shipped combat tables.

## What actually runs

| Capability | Current evidence | Equipment proposal boundary |
| --- | --- | --- |
| Two ranged weapon rows | [`Weapon` and `WeaponSpec`](../../../crates/strata-sim/src/combat.rs), [`weapons.ron`](../../../assets/combat/weapons.ron) | Only Carbine and Pistol exist. Burst, sniper and heavy chassis need new semantics and validated vocabulary. |
| Melee commitments and dodge | [`actions.ron`](../../../assets/combat/actions.ron), [`Combatant`](../../../crates/strata-sim/src/combat.rs) | Light, Followup and Heavy have contact windows, stamina costs and posture damage. A Guard profession branch is passive progression, not a block or parry action. |
| Pools and regeneration | [`actor.ron`](../../../assets/combat/actor.ron), `Combatant::hit` and `Combatant::step` | Actor defaults are 100 health, 40 shield, 100 stamina and 100 posture. Shields absorb damage before health. Shield regeneration is 8/s after 3 s without a hit. Ordinary health regeneration and supply use do not exist. |
| Projectile collision | [`strata-physics combat`](../../../crates/strata-physics/src/combat.rs) | Bolts sweep a sphere, stop at nearest obstruction and retain launch direction. The actor table supplies a common 120 m limit. No damage types, penetration, ballistic drop, heat or homing exist. Plasma and conventional are proposed families, not present resistance systems. |
| Skill effects and certifications | [`progression::apply`](../../../crates/strata-sim/src/progression.rs), [`registry vocabulary`](../../../crates/strata-registry/src/progression.rs) | Existing composition applies all Sets, then Adds, then Muls in graph order. It has no equipment source, item instance, expiry or inventory system. A weapon not gated anywhere in the graph defaults to open access. |
| Learn and forget | `CharacterSheet::learn` and `CharacterSheet::forget` in [`progression`](../../../crates/strata-sim/src/progression.rs) | Learning checks prerequisites, XP, budget and teacher. Forget checks dependents and combat. Learning itself has no combat argument. Equipment requires a consistent authoritative transaction gate. |
| Household | [`apps/game household`](../../../apps/game/src/household.rs) | Local-only saved characters carry sheet, weapon, two magazine counts, tint and last position. They do not persist health, injury, implants, item instances or buffs. Switching rebuilds full pools. A connected client has no household. |

The existing carbine is 24 damage every 0.24 s, 24 rounds and a 1.35 s
reload. The pistol is 15 damage every 0.15 s, 12 rounds and a 0.9 s reload.
Both have ideal unreloaded damage rate 100/s. Using a cycle of
`magazine * interval + reload` gives about 81.36/s and 66.67/s respectively.
This is an analytical convention, not observed hit rate. First-to-last-shot
time is `(magazine - 1) * interval`; travel, misses and scheduler boundaries
change actual results. A buffered melee follow-up starts at the outgoing
contact end, so adding full action durations does not reproduce chain timing.

## Composition and pool preservation

Proposed equipment composition starts from immutable installed chassis and
actor rows every time. It never applies another factor to already fitted rows.
Learned skills, item modules, implants and future active effects each contribute once
under a documented source order. Duplicate source ids are an error. A proposal
factor is a dimensionless multiplier of that chassis's own stock property;
whether a lower property is beneficial depends on its meaning. Cadence as an
interval and cadence as shots/s must never share one ambiguous field.

The eventual runtime model must name units, field direction, rounding, caps and
source order. It must check final values after every source is composed, not
only check each module's 0.65..1.35 range: several legal multipliers can produce
an illegal result. Reject nonfinite values, nonpositive durations, impossible
contact windows and integer magazine overflow. Define additive bonuses and
percentage bonuses separately. Avoid competing Set operations across sources.

Rebuilding stats preserves each live current pool as
`new_current = min(old_current, new_capacity)`; raising a capacity adds no
health, shield, stamina or posture. Deliberate healing and regeneration are
separate events. Death remains death. Lowering a capacity may discard excess,
but replacing that capacity cannot recover it. Unequipping, future buff expiry,
forgetting a box and save/load all obey this rule.

This differs deliberately from current `Combatant::fit`: a full pool follows
its new cap. Repeatedly lowering a health cap to the current value and raising
it again could heal through refitting. Equipment must not reuse that behavior
as its live-pool policy. New-character initialization may fill pools once.

## Ownership and authoritative changes

Proposed household ownership means one unique instance can be in shared stock
or assigned to one character, never both. Installed modules remain owned
instances, not free copies generated from schematics. Body implants belong to
one character. A benched character's loadout still reserves its items.
Schematics shared by the household do not grant certifications to its members.

Equip, remove, modify, learn, forget, implant installation and supply use are
authoritative transactions. Validate ownership, available stock, place,
certifications, prerequisites, combat state and costs before committing any
change. Commit item movement, resource payment and fitted state together, or
leave all unchanged. Repeated requests must not duplicate an item, spend twice
or grant a second restoration. A client's screen is a request surface, not authority.
This is a future integration requirement; the current local sheet and bench
write directly from Update and have no equipment networking.

Forgetting returns skill points and retains XP, but cannot preserve use of a
weapon whose certification was lost. Check dependent boxes first and refit
after success; keep the owned item but disable uncertified firing/equipping.
Apply the combat gate to learning as well as forgetting and workshop fitting.
Drawing an already owned complete weapon remains a timed field action under
the combat scheduler, as specified in equipment rules.
Do not add a fee to the decided free out-of-combat forgetting rule.

Persist current pools, injuries, implant assignments, cooldown expiry per character before permitting equipment-backed switching.
Switching and restarting must not fill pools, reset supply cooldowns or clone
magazines. Household stock is shared; cooldowns and bodily effects are personal.
The shared recovery-supply cooldown uses simulation-time expiry and survives a
bench switch and save/load. Current supplies create no active timed effects.

## Supplies and implants

The three implants and three supplies are provisional choices, not current
runtime mechanics. Each implant needs its beneficial actor property, its cost
or drawback, compatibility and installation rule. A bonus to posture capacity
or regeneration does not create guard. An implant must not grant a move or
certification implicitly.

The initial supplies restore 25 health in 3 s, 20 shield in 2.5 s or 30
stamina in 2 s. They share a personal 45 s recovery cooldown starting at
successful completion. They create no active buffs, temporary capacity bonuses
or reapplication/expiry behavior. Future timed effects require a separate design.

Use reserves one dose, cancels aiming and limits walking to 35 percent. Shooting,
attacking, sprinting, dodging or a confirmed damaging hit cancels use. Cancellation
releases the reservation without consumption, restoration or cooldown. Full pools
refuse use before reservation; a pool filled during use likewise releases the dose.
Successful completion consumes one dose, restores only missing capacity and starts
the shared cooldown atomically. Healing cannot revive a dead actor. Shield recovery
is not health healing and never supplies plasma ammunition. Neither restoration
nor interruption silently erases stagger. Carry limits are six ready doses total,
with at most five per type, as specified in [equipment rules](equipment-rules.md).

## Validation and acceptance scenarios

Static validation is evidence of a coherent package, not combat balance.
Check unique ids, references, slot compatibility, manufacturer/chassis coverage,
finite bounded factors and all required tradeoffs. Enumerate legal module
combinations to check composed bounds when the package defines them. Reports
must distinguish static passes, analytical illustrations and unrun runtime work.

| Scenario | Required future result | Evidence method |
| --- | --- | --- |
| Damaged actor cycles capacity implant/module and respecs | Current pools never increase without a healing event | Sim state scenario, including lowering then raising a cap |
| Forget last relevant certification | Item remains owned; drawing/firing is refused | Progression and equipment transaction scenario |
| Learn or workshop-fit while engaged or committed | Entire transaction refused; scheduled weapon drawing is a separate field action | Sim authoritative gate scenario |
| Two characters request the same item | Exactly one assignment; inventory count conserved | Household transaction and duplicate request scenario |
| Switch, restart or save/load during supply cooldown | Pools, charges and cooldown expiry retained | Persistence round-trip and fixed-clock scenario |
| Use health then shield or stamina supply | Shared 45 s cooldown refuses the next use without reserving/spending a dose | Completion and cooldown boundary scenario |
| Heal at full health, while staggered or dead | No overheal; stagger rules retained; no resurrection | Resource and action-state scenario |
| Maximum legal module/skill/implant combination | Final values remain valid and rebuild is idempotent | Exhaustive finite combinations plus modifier order scenario |
| Cover, muzzle obstruction and thin wall | Every projectile family respects declared collision rules | Existing physics harness extended for each new family |
| Pistol, burst, sniper and heavy at 4/12/30/60 m | Roles differ by measured openings, travel and commitments | Fixed-seed duel matrix, not relative indices |
| Equally invested melee and ranged against rusher, sentry and mixed encounter | Each has viable matchups and recoverable mistakes | Real sim/physics harness, damage taken, kill time, stamina/posture and reload metrics |
| New commando tree with prerequisite builds | Fits 250-point budget; no free bypass through equipment | Graph validation and explicit affordable build sheets |

For combat acceptance use the real sim and physics following
[`docs/balance/README.md`](../../balance/README.md). Run fixed-seed sweeps,
include both short bouts and fights long enough to expose reload, regeneration,
posture and supply opportunity cost, and report misses as well as theoretical
damage. Establish absolute chassis rows before setting parity bands. A proposal
index of 1.0 cannot establish equal weapons or equal professions.

When gameplay integration lands, add deterministic scenarios before tuning,
record prediction separately from human play, and run the repository's required
`just ci`. This design-only package does not claim those future scenarios pass.
