# Weapons for the Serein Basin

Design proposal, 2026-10-04. This is a first catalogue for discussion and later
data authoring, not implemented equipment or a claim of measured balance.
It follows [Veyra](../../SETTING.md), [progression](../progression.md), the
[combat slice](../combat/DESIGN.md), and the
[frontier catalogue](../frontier-life/catalogue.md).

The user has chosen equally viable melee and ranged combat, a heavy-weapon
Commando profession, pistol, burst and sniper ranged roles, plasma technology,
a few mainstream manufacturers with DIY adaptations, recognizable chassis
with controlled interchangeable modules, and procedural Ashlar meshes.
Conventional projectiles remain a candidate. The eight chassis, names,
manufacturer assignments, slots and handling below are agent proposals.
There is no new gameplay, generated artwork or engine work in this package.

## What exists today

The shipped [weapon table](../../../assets/combat/weapons.ron) contains only
`Carbine` and `Pistol`. These are combat enum names, not catalogue IDs. Neither
is automatically renamed or migrated by this proposal.

| Live unmodified table row | Damage / posture per hit | Shot interval | Bolt speed | Magazine / reload |
| --- | --- | --- | --- | --- |
| Carbine | 24 / 18 | 0.24 s | 38 m/s | 24 / 1.35 s |
| Pistol | 15 / 9 | 0.15 s | 46 m/s | 12 / 0.9 s |

The [action table](../../../assets/combat/actions.ron) supplies shared blade
actions, not three distinct melee weapon profiles: Light is 0.40 s, costs 18
stamina and deals 28 damage / 34 posture; Followup is 0.40 s, costs 18 and
deals 34 / 42; Heavy is 0.85 s, costs 32 and deals 55 / 80. The
[actor table](../../../assets/combat/actor.ron) starts actors at 100 health,
40 shield, 100 stamina and 100 posture, with a 120 m bolt travel limit.
Profession effects can modify these values.

These are authored baseline values, not suggested targets for the new roster.
The [contracts](../../../assets/combat/contracts.ron) explicitly record
unresolved duel length and blade approach balance. In particular, the recorded
open-ground 12 m blade approach loses before closing. Equal viability is a
design goal that this document cannot certify. New damage, cadence, heat,
spread and stamina numbers need a later balance pass and played evidence.

## An understandable supply chain

| Working maker and ID | Chassis language | Commercial place |
| --- | --- | --- |
| Kestrel, `veyra:maker.kestrel` | Squared receiver, protected controls, pale service panels, broad replaceable covers | Institutional procurement and widely stocked service spares |
| Morrow, `veyra:maker.morrow` | Slim continuous spine, recessed fittings, clean sight line, small adjustment collars | Precision equipment and specialist servicing |
| Bracken, `veyra:maker.bracken` | Exposed fastening points, blunt guards, generous access, repairable clamshells | Frontier workshops and rugged utility adaptations |

These are working names. Makers are suppliers rather than factions, and none has a universal damage or quality
bonus. Pelagic wardens can carry Bracken repairs; independent travelers can
buy Kestrel stock. DIY adaptations retain the base maker, chassis ID and
repair history. A fabricated grip or replacement sleeve is not a fourth maker
or an automatic combat upgrade.

The companion [catalogue.json](data/catalogue.json) is authoritative for the exact
draft records, compatibility and module factors. Slot tables below summarize
the exact draft module records. Stock-only slots
have no alternative record in this package.
Stock indices normalized to one are comparison scaffolding, not proof of
equal combat power across chassis. Multipliers are paper proposals, not live
combat modifiers or measured balance.

Each weapon has two or three functional slots. A slot accepts only compatible
options, one at a time, including its stock part. Every replacement has a
drawback. Fabrication quality and service life remain future design topics;
this package adds no durability loss. Cosmetic panels and finish are separate
from functional slots. No random combination changes grip anchors, handedness,
action family or weapon category.

## The proposed first eight

Useful distance bands below are qualitative encounter intentions, not hit
cutoffs. Physical dimensions are coarse mesh and animation envelopes in metres,
not hitbox definitions or verified production measurements.

### Service pistol

`veyra:weapon.service_pistol`, Kestrel. A compact pulse sidearm with a short
square slide-like receiver and a battery cassette in the grip. It rewards
moving between nearby cover and frequent controlled shots. A pistol specialist
should be able to use it as their primary, with mobility and fast servicing
compensating for limited reach and small capacity. Sustained close pressure is
its role; easy long-range precision and rifle-sized magazines are excluded.

Approximately 0.24 m long, 0.15 m high and 0.045 m wide. The firing hand owns
the grip; the support hand cups it in aimed fire. Use a pistol aim family,
visible wrist recoil and a grip-cassette reload. A free support hand is a pose
possibility, not authorization for dual wielding or simultaneous tool use.
Sold at Port Serein as reliable traveler equipment; a serviced used example
is a plausible early purchase.

| Slot | Draft options and trade |
| --- | --- |
| Sight | Stock assembly; `veyra:module.compact_optic`: lower aim spread, higher equip time |
| Feed | Stock assembly; `veyra:module.pistol_extended_cell`: higher capacity, higher reload time |
| Grip | Stock assembly; `veyra:module.pistol_stable_grip`: lower recoil recovery, higher equip time |

### Burst rifle

`veyra:weapon.burst_rifle`, Kestrel. The proposed successor role to the live
compact carbine, without treating today's automatic cadence as a burst
implementation. A deliberate trigger request fires a fixed three-pulse burst,
followed by a recovery interval. It rewards short midrange exposure and a
stable line during the burst. Leading three pulses is harder than landing one;
missed bursts waste supply, and close circling punishes the commitment. No
full-auto conversion in this catalogue.

Approximately 0.76 m long, 0.24 m high and 0.075 m wide. Shoulder stock,
rear pistol grip and forward support grip are fixed anchors. Burst recoil
must show three impulses and recovery without animation moving the reticle.
The feed cassette enters below the receiver. Available through ordinary
equipment dealers and escort work, rather than institutional allegiance.

| Slot | Draft options and trade |
| --- | --- |
| Sight | Stock assembly; `veyra:module.compact_optic`: lower aim spread, higher equip time; `veyra:module.adapted_morrow_optic`: lower aim spread, higher equip time, lower aim move speed |
| Feed | Stock assembly; `veyra:module.rifle_extended_cell`: higher capacity, higher reload time |
| Fore end | Stock assembly; `veyra:module.rifle_cooled_fore_end`: lower repeat interval, lower aim move speed |

### Precision rifle

`veyra:weapon.precision_rifle`, Morrow. A long, narrow conventional cartridge rifle with a
recognizable continuous top spine. The sniper role is deliberate long-range
placement, clear sight lines and a significant wait between powerful shots.
Misses and movement after firing cost opportunities. Small capacity, slow
reloads and poor close tracking prevent it replacing the pistol or burst
rifle. No homing, target lock, through-wall shots or arbitrary armor immunity.

Approximately 1.05 m long, 0.25 m high and 0.075 m wide. Shoulder contact and
a forward support hand require their own reach envelope. Show a settling
pose, pronounced recoil recovery and a side-access cartridge magazine. A deployed
rest is a future stance dependency, not a deployable system promised here.
Bought from a specialist dealer or commissioned after survey and escort work;
it is an alternative specialization, not the end of a universal upgrade ladder.

| Slot | Draft options and trade |
| --- | --- |
| Sight | Stock assembly; `veyra:module.precision_long_optic`: lower aim spread, higher equip time |
| Feed | Stock assembly; `veyra:module.precision_compact_feed`: lower capacity, lower reload time |
| Stock | Stock assembly only |

### Scattergun

`veyra:weapon.scattergun`, Bracken. A short, broad receiver and blunt clustered
muzzle establish a close spread weapon. The proposed delivery is conventional pellet cartridges in a detachable box
magazine. This technology assignment remains a proposal, not a user decision
or a shipped ammunition system. Close
hits can deliver strong health pressure, but spread makes damage unreliable
as separation grows. Its limited feed and slow refill create an opening for
a blade user. Per discharge, posture pressure should remain below a committed
heavy cutter hit, and pellets must not each apply a full stagger.

Approximately 0.69 m long, 0.25 m high and 0.10 m wide. Two hands and a shoulder
stock, with a broad forward grip and large recovery movement. Show a bottom-mounted detachable box magazine reload. Tube feeds and
per-shell loading with their cancellation rules are outside this chassis. Frontier dealers and workshop repair
commissions supply it. It belongs to ordinary close ranged training, not the
Commando's heavy-weapon gate.

| Slot | Draft options and trade |
| --- | --- |
| Muzzle | Stock assembly; `veyra:module.scatter_wide_muzzle`: higher aim spread, lower repeat interval |
| Feed | Stock assembly; `veyra:module.scatter_extended_feed`: higher capacity, higher reload time |
| Grip | Stock assembly only |

### Support projector

`veyra:weapon.support_projector`, Kestrel. The Commando's heavy plasma weapon
has a thick emitter housing, visible heat sink and large side-mounted feed.
It sustains pressure on a visible lane through a sequence of substantial
plasma pulses. Heat limits the firing string and cooling creates an explicit
vulnerable interval. It needs both ammunition and heat management; neither
is a disguised stamina tax. Bracing improves control at the cost of movement,
and a flanker can exploit its slow turning and readying. Direct impacts are
the first proposal. Splash, suppression debuffs and destructible cover are
excluded until separately designed.

Approximately 0.98 m long, 0.31 m high and 0.20 m wide. Rear firing grip,
forward load-bearing handle and stock or hip brace have fixed anchors. Its
mass needs a distinct carry, aim, vent and reload family; the rifle animation
cannot simply be reused at larger scale. No loose ammunition belt or backpack
hose in this first envelope. Specialist stock or an escort-service commission
provides access, with costly but obtainable maintenance supplies. Ownership
and profession competence are separate: Commando training carries safe,
effective sustained handling. Exact prerequisites remain for progression design.

| Slot | Draft options and trade |
| --- | --- |
| Cooling | Stock assembly; `veyra:module.support_cooling_jacket`: higher cooling rate, higher brace time |
| Feed | Stock assembly only |
| Support | Stock assembly; `veyra:module.support_stable_rest`: lower recoil recovery, higher brace time |

### Field blade

`veyra:weapon.field_blade`, Morrow. A slim physical cutting edge with a clear
guard and visible mechanical service collar. It specializes
in inexpensive quick contacts, short recovery and choosing when to disengage.
Light health pressure is dependable once close, but reach and heavy posture
pressure trail the cutter. A stamina-efficient blade still cannot cancel every
miss immediately, ignore walls or cross open ground by repeated lunges.

Approximately 0.72 m overall, including a 0.17 m grip, with a 0.07 m guard.
One primary hand and an occasional supporting hand near the pommel; preserve
guard clearance in cuts and holstering. Short cut and return trajectories
should retain readable contact windows. Early explorer equipment from field
dealers, with a locally replaced edge a plausible first fabrication service.

| Slot | Draft options and trade |
| --- | --- |
| Edge | Stock assembly only |
| Grip | Stock assembly; `veyra:module.blade_balanced_grip`: lower attack stamina, lower hit posture |

### Heavy cutter

`veyra:weapon.heavy_cutter`, Bracken. A two-handed industrial cutting tool
adapted into a fighting weapon, with a thick asymmetric head and broad guard.
Its identity is a costly, telegraphed posture-breaking commitment, then a
decisive follow-up if the opponent stays. Long anticipation, stamina demand
and recovery make missed swings punishable. Light actions are setup attacks,
not a field blade's rapid sequence with free extra reach. Being derived from
a tool does not grant mining, door cutting or terrain destruction mechanics.

Approximately 1.02 m overall, with a 0.34 m two-hand grip and 0.14 m head
width. Both hands remain on stable anchors through the contact arc. Heavy
movement must express mass without moving the gameplay root or making the
head pass through obstacles. Sold refurbished in inland service towns or
commissioned from a fabricator using ordinary replacement parts.

| Slot | Draft options and trade |
| --- | --- |
| Head | Stock assembly; `veyra:module.cutter_reinforced_head`: higher hit posture, higher recovery time |
| Power | Stock assembly only |
| Grip | Stock assembly only |

### Impact baton

`veyra:weapon.impact_baton`, Bracken. A blunt physical impact tool with a compact
head, protected grip and conspicuous safety collar. It trades blade health
damage for precise close posture pressure and manageable recovery. Its shorter
reach demands careful footwork, and a healthy unbroken target takes longer to
finish. It is not guaranteed nonlethal, an electrical stun wand or an infinite
interrupt chain. Repeated pressure must leave the target meaningful escape and
retaliation opportunities.

Approximately 0.52 m long, with a 0.16 m grip and 0.075 m head. Primarily
one-handed; short strikes and a guarded recovery distinguish it from blade
cuts. A braced heavy contact can use the support hand without becoming a
two-handed cutter stance. Available as plant safety equipment and settlement
escort gear; DIY work commonly replaces a head shell or worn grip.

| Slot | Draft options and trade |
| --- | --- |
| Head | Stock assembly; `veyra:module.baton_weighted_head`: higher hit posture, lower hit damage |
| Grip | Stock assembly only |

## Keeping the roles viable together

The pistol, burst rifle and precision rifle are three primary-capable ranged
choices, not low, medium and high tiers. The scattergun occupies a fourth
close ranged role. The projector earns its separate Commando profession
through sustained lane pressure, heat and handling commitments; it cannot be
a scattergun with a bigger damage number. Commando must not confer blanket
superiority with ordinary guns. The older progression document's Marksman
branches do not yet implement this division, and the name Blade does not
settle whether baton training belongs there or in a broader melee tree.

Melee needs routes into contact through cover, movement and readable firing
openings. Ranged opponents need distance, tracking and punishment for missed
commitments. Assess open ground, broken terrain and close service interiors
with comparable profession investment and equipment quality. Do not achieve
parity solely by granting blades enough damage to win a standing-target test.
The field blade rewards repeated small openings, the cutter rewards one large
opening, and the baton rewards controlled posture pressure. None gets passive
bullet immunity, forced target lock or stamina-free dodge chains.

Later tuning should check time to first contact, damage taken while closing,
miss recovery, ammunition and stamina margins, posture-break frequency,
moving accuracy and outcomes at several starting separations. A scattergun
hit and projector string must not delete a fresh melee opponent before any
answer is possible. Conversely, a missed melee heavy must give a ranged
opponent time to answer. Harness evidence and human play remain separate.

Run solo mixed encounters with a stock weapon and no implants or supplies.
Precision and support specialists need a recoverable close-pressure response
within their own kit; a secondary weapon is an optional investment, not a
hidden entry fee for the profession. In particular, evaluate defensive
unbraced projector fire as well as its ideal prepared lane. Test the baton
against an opponent attempting escape after every posture break, and reject
an inescapable repeated stagger even if its average damage appears modest.

## Mesh and module contract

Ashlar-authored procedural meshes should preserve each chassis' silhouette,
receiver spine and grip spacing. Modules use named sockets for sight, feed,
grip, muzzle, cooling or head as appropriate; stock parts occupy those same
sockets. A maker palette changes panels, paint and fastening language, not
the functional dimensions or attack timings. Wear belongs at grips, cassette
latches, edge tips and heat outlets. Restrained bevels and convincing materials
follow the setting's asset direction.

Module envelopes must clear fingers, the sight line, stock contact, reload
paths and holsters. Do not randomly scale a grip or shift an emitter. Record
hand, muzzle or contact-tip, feed extraction and stow anchors before animation
authoring. The equipped configuration supplies a stable silhouette and bounds;
collision and hit tests remain separately authored. Decorative fins cannot
extend melee reach, and a muzzle sleeve cannot silently move the firing origin
through cover. Procedural variation is deterministic from saved configuration,
with no gameplay authority delegated to a rendered mesh.

Each stock chassis remains the recognizable base when fitted with its largest
legal modules. DIY covers may look patched but must satisfy the same socket
and clearance contract. No generated art, Ashlar recipe implementation or
animation production is included here.

## Deliberate exclusions and review points

Eight bases are sufficient for a first catalogue. Grenades, launchers,
flamethrowers, dual wielding, shields, exotic damage technologies, homing,
beam weapons and additional conventional-ammunition variants would introduce extra rules
before these identities have been tested. The existing pulse carbine remains
the prototype baseline, not a ninth catalogue item.

Review plasma burst delivery and conventional precision ballistics together:
visible pulses are readable, while conventional shot travel, recoil and range
still need explicit rules for the sniper role. Review the
scattergun and baton together: both need close usefulness without removing
the field blade's reason to exist. Review the projector's handling burden
against actual solo travel as well as team fights, so Commando remains a
viable specialization for a household character. Conventional ammunition,
profession prerequisites, consumption economy and quantitative tuning remain
explicit follow-up decisions.
