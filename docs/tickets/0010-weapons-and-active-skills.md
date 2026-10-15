# 0010: Weapons with a role, melee as a weapon, active skills

Status: built 2026-10-14 (steps 1 to 9). Written after ticket 0009, when the
user asked to "discuss skills/combat mechanics": "tweak weapons - pistols
primarily single shot medium range. carbines burst centric, rifles long range
sniping", "design the ui/ux of active skills, how to select them, how the
professions unlock them", and "codex made a bunch of icons for us to use as
well". The outline was put to them in two rounds and the decisions under
"Decided by the user" are theirs; everything else is a default.

Singleplayer still holds. Combat is not in the replication protocol, so
nothing is added to it; `world_version` and `GENERATOR_VERSION` do not move.
The combat tables' own version does, as it must when a number changes.

## What is wrong with combat as it stands

- **The two guns are one gun.** Pistol and carbine both fire while the
  button is held, both do 100 damage a second, and neither loses anything
  with distance. Range is one number for every actor (120 m).
- **Nothing rewards a long shot.** The sentry fights at 9 to 32 m and a
  hostile wakes at 11 m.
- **Melee is two keys under any gun.** The light and heavy cut work with a
  carbine in hand, there is no blade item, nothing to craft, and no guard.
- **There are no active skills.** A box changes a number or opens a cut.
  Nothing is chosen, slotted or shown, and Codex's 96 painted ability
  squares are bound to nothing.

## 1. Guns with a role

A ranged row of `assets/combat/weapons.ron` gains a fire mode, a range with
falloff, and a cone that grows as it fires. Starting numbers, for the harness
to tune:

| | Fire | Damage | Full damage to | Fades to half by | Bolt | Character |
| --- | --- | --- | --- | --- | --- | --- |
| Pistol | one shot a press, 0.32 s apart at best | 30 | 30 m | 60 m | 60 m/s | accurate from the first shot, quick to draw |
| Carbine | a burst of 3 a press, 0.42 s between bursts | 3 x 20 | 20 m | 45 m | 45 m/s | the third round wanders; the most damage up close |
| Rifle | one shot, 1.4 s apart, magazine of 4 | 90 | 40 to 250 m | none inside its range | 140 m/s | tight only when aiming and still; a scope |

- **Fire mode** is data: `Single`, `Burst(count, gap)`, `Auto`.
- **Full auto on the carbine, at a price.** Holding the trigger past the
  burst keeps it firing at the burst's cadence. Every further round widens
  the cone, up to about three times, and it settles only once the trigger is
  let go. The burst is the accurate way; the hold is for a rusher at arm's
  length.
- **Falloff** is a share of damage by distance flown, per weapon. Range
  moves from the actor's table to the weapon's row.
- **Bloom** is per weapon: what a round adds to the cone, the cap, how fast
  it settles. Moving widens the rifle's cone a great deal and the pistol's
  hardly at all.
- **The scope.** Aiming the rifle narrows the view, by the weapon's own
  figure. A fitted sight changes it, which is what the sight mount is for.
- **Crafting follows.** The carbine's signature line already scales its
  `interval`, which becomes the gap between bursts. The rifle gets a
  signature of its own (its cone at rest).

The harness gets range presets and a contract a weapon: each has the
shortest time to kill in its own band and loses outside it.

## 2. Melee as a weapon

A melee weapon is a weapon the player chooses to carry, as in SWG and Dark
Souls: an item, bought or made, with quality lines, parts and wear.

- **Two weapon slots, any two weapons.** Today the hands are a carbine's
  slot and a pistol's. They become two free slots (keys 1 and 2): two guns,
  a gun and a blade, or two melee weapons.
- **No cuts with a gun in hand.** A gun's only melee is a butt strike, an
  ability like any other.
- **The mouse mirrors the gun.** Left is the light chain where a gun fires.
  Right is guard where a gun aims.
- **Guard and parry.** A raised guard takes a share of a blow or a bolt and
  pays for it in stamina; out of stamina, the guard breaks into a stagger. A
  guard raised just before the hit lands is a parry: a blow's owner takes
  heavy posture damage, a bolt is turned aside.
- **Movesets belong to the weapon.** The cut timings leave the one global
  table for a row a class: reach, the chain's contact windows, stamina,
  damage, posture damage, the guard's share and the parry's window.

| Class | Reach | Light | Guard takes | Parry window | Good at |
| --- | --- | --- | --- | --- | --- |
| Blade (one hand) | 1.7 m | 28 damage, 0.40 s, 18 stamina | 60 % | 0.20 s | pressure, parry and reply |
| Greatblade (two hands) | 2.2 m | 46 damage, 0.70 s, 30 stamina | 80 % | 0.10 s | breaking posture, trading blows; not staggered mid swing |
| Glaive (polearm) | 3.0 m | 32 damage, 0.55 s, 22 stamina | 50 % | 0.14 s | holding distance, several targets |

- **Parts.** Two mounts a melee weapon, grip and edge, and four parts to
  start (two a mount), fitted on the screen that exists. A melee weapon's
  quality lines are edge (damage), balance (stamina a cut), temper
  (durability) and moddability (which mounts are open).
- **Closing on a gun.** The guard stops bolts, the parry turns them, each
  class has one closing move, and the dodge's safe window is there already.
  The harness holds a contract that melee against a gun from 25 m is a fair
  fight.
- **Art.** Each class needs a recipe in `veyra-weapons`, a picture, a model
  and a schematic. The body has one set of blade clips, so the three classes
  share them, retimed: they play differently and look alike until new clips
  exist. This is known art debt.

## 3. Active skills

- **What one is.** A row of `assets/combat/abilities.ron`: its icon, the
  weapon classes it works with, its stamina cost, its cooldown, its
  commitment (wind-up, length, when a dodge may cut it short) and one effect
  from a small fixed set the sim implements: a modified shot or burst, a
  melee strike with its own contact row, an effect on oneself, a status on
  whoever it hits, a movement.
- **Stamina and a cooldown.** One pool, so an ability competes with the
  dodge and the guard.
- **Statuses** are data too: a name, an icon, a length and what it scales.
  Staggered exists. New: slowed, shield broken, focused, fortified, disarmed.
- **A refusal is said.** Short of stamina, on cooldown, wrong weapon: the
  slot flashes and says which, by the one rule the request also asks.

The first set, every one with a painted square in the library:

| Ability | Works with | What it does |
| --- | --- | --- |
| Precision shot | pistol | a short wind-up, then one bolt with no cone and over twice the damage |
| Ricochet | pistol | the bolt jumps to a second target nearby |
| Disarm | pistol, blade | the target cannot attack for a moment |
| Aimed burst | carbine | five rounds with no bloom |
| Suppressing fire | carbine | a long fast burst; whoever it hits is slowed |
| Explosive shot | carbine | an area of posture damage |
| Charged shot | rifle | hold to charge; breaks through a shield |
| Armour break | rifle, greatblade | the target's shield will not recover for a while |
| Focus | rifle | the cone closes for a few seconds, moving or not |
| Butt strike | any gun | a short stagger at arm's length |
| Heavy cut | each melee class, by its own row | the slow posture breaking blow |
| Thrust | blade, glaive | a lunge that closes the distance |
| Reflect shot | blade | for a moment every guarded bolt goes back |
| Sweeping cut | greatblade, glaive | an arc that hits everyone in front |
| Kinetic absorb | greatblade | no stagger for a few seconds |
| Vault | glaive | a leap over the pole that closes the distance |
| Rapid reload | utility | a reload at once |
| Shield recharge | utility | the shield starts to recover now |
| Combat slide | utility | a low fast slide that keeps the weapon up |

Each class has four or five and slots three, so the choice is real.

## 4. Choosing and seeing them

- **A bar per weapon.** Three slots that belong to the weapon class in hand
  and change when the other is drawn, and two utility slots that stay.
- **Keys.** Q, E and T for the weapon's three; 3 and 4 for utility. 1 and 2
  stay the weapons, R the reload, Alt or Ctrl the dodge, F, G and V as now.
- **The HUD.** A bar at the bottom centre: painted squares, the key on each,
  a sweep for the cooldown, grey when stamina is short. The two weapons sit
  to its left as symbols with the one in hand lit. Statuses are small squares
  by the vitals, the player's and the target's.
- **The loadout.** A tab on the character sheet: each weapon class the member
  is certified on, its three slots, and the abilities known for it. Changed
  anywhere out of a fight, saved per member, and a newly learned ability
  takes the first empty slot.
- **The sim owns it.** The bar is a component, a change is a request in an
  inbox with one `can_slot` rule, and the screen is a mirror, as everywhere.
- **The icons.** Codex's library is committed with this work. An ability's
  row names its icon by catalogue id and a test holds every row to a square
  that exists. Symbols take the weapon classes, the professions and the
  empty slots.

## 5. How professions teach them

A box gains two effects: `Grant(ability)` and `Ability { id, field, op,
value }`, read through the one fold the Surveyor and the Fabricator use. A
branch of four is: a grant, a passive, a second grant, something that
sharpens one of them. The sheet draws the painted square on a granting box.

| Tree | Branches | What they give |
| --- | --- | --- |
| Marksman | Pistol, Carbine, Steadiness, Field craft | each gun's certificate and abilities; butt strike and the slide; reload and shield recharge |
| Blade | One hand, Greatblade, Glaive, Guard | each class's certificate and abilities; the guard's share, the parry's window, stamina |
| Ranger (elite) | Long shots made real; the other three stay empty | the rifle's certificate, charged shot, armour break, focus |

- The Blade tree's branches are renamed in what the player reads and the box
  ids are kept, so nothing is refunded and no save is touched.
- Ranger needs what it needs today (Marksman novice and Carbine 1, Surveyor
  novice, Routes 1 and Reading 1): 24 points in all, which Ilse Marrow is
  most of the way to.
- Wilderness XP, which nothing earns today, comes from rifle damage and from
  any hit landed past 50 m.

## Steps

One workflow a step, commit a step, the full CI once at the end.

1. **Guns with a role.** Fire modes, falloff, bloom and the full auto
   price; pistol and carbine retuned; range presets and contracts.
2. **Melee in the sim.** Two free weapon slots, three melee classes with
   their rows, guard and parry, cuts gone from a gun; items with stand-in
   art; the save upgraded.
3. **Melee made.** Recipes, pictures, models, two mounts and four parts,
   schematics, and a blade in the first day's stock.
4. **The rifle.** Row, recipe, item, schematic, a scope part, the scoped
   view.
5. **Abilities in the sim.** The data, the commitment, cooldowns, statuses,
   the two new effects, the bar and its requests; the harness presses them.
6. **The bar, the statuses and the loadout tab**, with the icon library
   bound and committed.
7. **The trees.** Marksman and Blade rewritten, Ranger's Long shots real,
   trainers, Wilderness XP.
8. **Opponents and tuning.** A marksman who holds 40 to 80 m, a hostile that
   guards, every contract held, and a page that says what each weapon and
   ability is worth.

## As built

### Step 1: guns with a role (2026-10-12)

Stills in `docs/design/combat/`: `step1-fitting.png` (carbine: Fire "Burst of 3,
Hold for full auto", range, reach), `step1-pistol.png` ("Single shot"),
`step1-rest.png` and `step1-hold.png` (reticle tight, then about twice as wide a
second into the hold) and `step1-controls.png` (LMB reads "Burst (hold: full auto)").

Built: the row's mode, hold, bloom, move_spread, range and falloff; the
scheduler in `Combatant::step`, with a press latch of the actor's `buffer`
(0.15 s) so an early press fires when the weapon is ready and a second click
during a burst is the next burst, not a hold; the bolt's falloff and death at
the row's range; `bolt_range` removed; the sentry's `Single` override; table
checks for every dial (the sentry's mode too); the range presets, five bands,
`--hold-fire`, and the card and fitting lines worded from `WeaponSpec::facts`.

| row | mode | interval | dmg | magazine | spread/aim | bloom (shot, cap, wait, settle) | range | falloff |
|---|---|---|---|---|---|---|---|---|
| Carbine | Burst 3 @ 0.09, hold 0.12 (+0.4) | 0.42 | 24 | 24 | 4.0/2.0 | 0.25, 4.0, 0.15, 6.0 | 70 | 25 / 50 / 0.55 |
| Pistol | Single | 0.32 | 24 | 10 | 2.4/1.0 | 0.6, 2.5, 0.20, 4.0 | 90 | 30 / 60 / 0.60 |

Presets (20 bouts, damage a second, hit share): pistol 10 m 68.2 (1.00), 30 m
43.5 (0.62), 60 m 7.7 (0.19); carbine 10 m 109.1 (1.00), 30 m 27.9 (0.29), 60 m
4.8 (0.08); held 8 m 137.2 (0.83), held 30 m 15.0 (0.10), burst 8 m 113.5 (1.00).

Bands (all chosen): pistol vs carbine at 10 m 0.63 (0.55 to 0.90); 30 m 1.56
(1.00 to 1.60); 60 m 1.67 (1.30 to 4.0); held vs burst at 8 m 1.21 (1.15 to
1.80); at 30 m 0.54 (0.2 to 0.90). Every older chosen band holds.

Decided for the user and easy to change (`assets/combat/weapons.ron`):
carbine damage 24, bloom shot 0.25, hold (0.12, 0.4), falloff (25, 50, 0.55);
pistol damage 24, bloom (0.6, 2.5, 0.20, 4.0), falloff share 0.6, magazine 10
(from 12); `opponents.ron` version 2. The brief's starting numbers were moved
inside the step's limits to hold the bands.

Left rough: the 30 m and 8 m margins are thin (1.56 of 1.60, 1.21 of 1.15); at
60 m neither gun is worth firing (about 19 s against 30 s to drop the dummy)
until the rifle exists; the pistol's figure is read from a hand at the weapon's
limit, a paced hand keeps a tighter cone and may land above the 30 m band; the
fitting table's long fact values sit under both columns; the item card keeps
"Shot or burst pause" where no weapon is known; the sheet's `interval` word
follows the row's mode.

### Step 2, first half: melee as a carried weapon (2026-10-12)

Stills (`docs/design/combat/step2a-*.png`, read before the step closed):
`blade` and `glaive` (the body from behind, two chips, the glaive's longer
shape), `stores` (a pinned blade, "Take in hand 2"), `fitting` (a glaive in
hand: its six figures and "No mounts" where the mounts go; Tovan has Footwork
1, no member has Breaking 1, so the greatblade is shown by tests). No refusal
still: the game has no flag to press Q and the brief allowed leaving it out.

Built: `Weapon` gains Blade, Greatblade, Glaive; `assets/combat/melee.ron`
(version 1, in `CombatTables`' version) scales the Light, Followup and Heavy
rows by class after the sheet and before the fitted row's stamina, and a Blade
reads the old bits (tested over every Blade box). A `Combatant` has two slots
and `CombatIntent.weapon` names one. A gun in hand refuses a cut once a press
(`Refusal::NoBlade`), including a cut queued across a draw; a melee weapon
cuts on the trigger's edge and chains. Hands take any weapon, never two of a
family (`SameFamily`). Three items, bought at Pelagic's board, two blades in
the first store, worn by their cuts. `--in-hand`, the HUD chips, the controls
card, Take in hand 1 or 2 and the fitting screen's cuts are in.

| class | reach | light dmg / s / stamina | heavy dmg / s / stamina | swing_posture |
|---|---|---|---|---|
| Blade | 1.7 m | 28 / 0.40 / 18 | 55 / 0.85 / 32 | 1.0 |
| Greatblade | 2.2 m | 46 / 0.70 / 30 | 91 / 1.15 / 48 | 0.3 |
| Glaive | 3.0 m | 32 / 0.54 / 22 | 61 / 1.02 / 38 | 1.0 |

Against the dummy (20 bouts, light chain; no Heavy is ever swung, since
stamina is short at the third opening or the dummy has fallen): damage a
second 80.7, 85.6, 80.7; time to kill 1.74, 1.64, 1.74 s; stamina low 10.0,
11.3, 14.0; posture damage a second about 107, 114, 87. The Greatblade does
the most posture, but not the least damage a second. The six bands are
`chosen: false`. The 30 presets that existed print byte-identical summary rows
before and after, including every cutting one (diffed over all 30).

Decided for the user and easy to change: every factor and `blade_reach_m` in
`assets/combat/melee.ron`; signature on `edge` 0.80 to 1.10 on each item file;
`worst` in `assets/crafting/wear.ron` carries `HitDamage: 0.8`, applied to
melee weapons only (a gun's bolts do not soften); two of a family cannot be
carried; Q stays the heavy, E is unbound; the Greatblade is certified by
blade.breaking.1 and the Glaive by blade.footwork.1 (the Blade by
blade.novice) until the trees are rewritten; drawing an empty slot or the slot
in hand is dropped silently; a gun new to a slot arrives with a full magazine;
`--in-hand` sends a displaced weapon to a free place in the store.

Left rough: the blade is one thin cube for all three classes (models are step
3); a class in hand that the sheet stops certifying refuses with the gun
refusal's words; no balance path varies `melee.ron`; the class presets do not
reach a Heavy (a policy that waits for stamina would); the in-hand table of
the stores screen does not say which weapon a Take would put away; the
`s2a-refused` still is not taken.

### Step 2, second half: guard and parry (2026-10-12)

Stills (`docs/design/combat/step2b-*.png`, read): `guard` (blade across the
body from behind, row reads GUARD), `rest`, `blocked` (`--script-progression
--script-hurt 1 --hold-guard 60`, "Blocked" over the player), `card`. No side
camera exists, so none is from the side.

Built: aim with a melee weapon holds a guard (Ready only, aim pace,
`guard_regen` stamina regen). A hit is a `Blow` with direction and kind;
`guard_outcome` is the one rule. The parry window opens only on a raise the
player made, so a held button through a cut never parries. A parried cut lands
its riposte on the swinger. Client: pose (`pose_guard`), the three words, a
parry as the metal impact plus the marker chime, no hurt vignette on a parry,
GUARD row label, card and fitting lines, "Take in hand N, puts away X".
Harness: `GuardAndCut`, `Profile::Post`; only the three class dummy presets
moved (33 older presets byte-identical).

| class | share | arc | stamina | posture | parry s | riposte |
|---|---|---|---|---|---|---|
| Blade | 0.60 | 70 | 1.40 | 0.40 | 0.20 | 70 |
| Greatblade | 0.80 | 80 | 0.70 | 0.20 | 0.10 | 90 |
| Glaive | 0.50 | 60 | 1.70 | 0.50 | 0.14 | 60 |

Presets (win, taken): `guard-vs-sentry-12m` 0.00, 119.9; `parry-vs-sentry-12m`
0.00, 0.0 (control brace 0.00, 140.0); `blade-guard-vs-rusher` 1.00, 11.0;
`blade-parry-vs-rusher` 1.00, 7.15 (control `blade-vs-rusher` 1.00, 27.5);
`greatblade-guard-vs-rusher` 1.00, 5.5; `blade-guard-vs-carbine-25m` 0.00, 140.
Bands: `guard-earns-its-stamina` 0.45 (0.2 to 0.75); `a-parry-turns-the-rusher`
1.00 (0.85 to 1.0, vacuous: the plain blade wins too); `a-parry-beats-a-block`
0.65 (0.0 to 0.9). Unchosen: 0.35 parries a bout, both 25 m bands 0.00, class
dummy bands 38.3, 32.5, 29.7 a second.

Decided for the user and easy to change: the table above, `parry_rest` 0.5 and
`guard_regen` 0.35 (`assets/combat/melee.ron`); the post's 300 health
(`opponents.passive_health`); the sentry pair stands and never cuts (cutting
gave 7 percent of the control's damage, under the 0.2 floor); opponents do not
guard yet.

The stamina column was raised after the review (from 0.50, 0.40 and 0.60),
which found a block cost about 8 stamina of 100. Now the rusher's heavy costs a
Blade about 22 and a Greatblade about 15, the guarding blade ends its fight at
14 stamina (it was 29) and the greatblade at 5, and a blade walking at a
carbine from 25 m has its guard broken three times. At 1.10 the greatblade's
guard broke in a loop against the rusher, so it is the cheap blocker at 0.70.
No band's figure moved.

Left rough: a parry never staggers a fresh attacker alone;
against a bolt at melee range a parry is a block; the riposte is scaled by the
swinger's own `swing_posture`; the pose is tuned by eye; the sentry band reads
the 30 s cap.

### Step 3: melee made (2026-10-12)

Stills (1280x720, `docs/design/combat/step3-*.png`): blade, greatblade, glaive
in hand (from behind only: no flag orbits the camera), fitting (hooked edge as
the candidate, AFTER column shown), diagram (greatblade, power mount shut),
crafting (service blade as a smith with Edges 1), sheet (Morrow contact sheet).

Built: the three items stand on chassis with baked picture and model; a new
`reach_glaive` chassis (2.14 m polearm); seven parts with schematics; ten
schematics taught by Edges 1 to 4; `RecoveryTime` live on a blade (tail only,
bit-identical at one); `fitting::live_on` per weapon; the hand draws the baked
model at true size; the fitting screen is the gun's screen on a blade;
`content --weapons-only`; `--learn`; harness presets `bought-`, `made-`,
`hooked-blade-vs-dummy`.

| weapon | chassis | mounts (opens at) | parts |
|---|---|---|---|
| service blade | field_blade | edge (open), grip (400) | hooked edge: posture 1.08, recovery 1.06; balanced grip: stamina 0.9, posture 0.94 |
| yard greatblade | heavy_cutter | head (open), grip (open), power (600) | reinforced head: posture 1.12, recovery 1.1; wrapped grip: stamina 0.96, equip 1.03; compact cell: capacity 0.85 (not live), equip 0.95 |
| pier glaive | reach_glaive | edge (open), grip (400) | broad edge: damage 1.08, stamina 1.08; counterweight: recovery 0.92, equip 1.06 |

Schematics and boxes: Edges 1 service blade, hooked edge; Edges 2 balanced
grip, pier glaive; Edges 3 yard greatblade, reinforced head, broad edge,
counterweight; Edges 4 wrapped grip, compact cell. A registry test holds them.

Bands: `a-made-edge-shows` (chosen) 1.247 in [1.08, 1.40], a kill-time ratio
that steps by whole cuts (9 against 8); the per-cut gain is 1.126, and the
high was widened from 1.25 so the step does not fail a band whose point is
its floor. `a-hooked-edge-breaks-posture` (chosen: false, measured
2026-10-12) 1.21 of the post's lowest posture depth, at 11.06 s to 11.21 s.

Review fixes: the counterweight's box and slot id, the blade's durability in
cuts, the dodge clip no longer warped by a recovery part, `fitting::live`
private, `--fitted` doc, glaive and attachment tests.

Decided for the user and easy to change: the glaive's shape and its two
parts' figures; which box teaches what; the mounts' thresholds; the item names
and makers kept (Pelagic Works); the made-edge band's high.

Pictures (fixed after the review): the field blade and the heavy cutter were
written flat across the recipe's frame, so the side view baked a hairline, the
hooked edge read the same as the standard one and the store icons were
slivers. `veyra_weapons::geometry::stood` now stands both recipes on their
edge, guard and all, as the glaive is written: the three lie the same way in
the hand (the frame's -Y leads a cut), the pictures show the flat, and the
hooked edge is a picture of its own. The stills were retaken on that bake.

Left rough: the in-hand stills are from behind only, where a held blade is
seen along its edge; the heavy cutter reads as a cleaver more than a great
blade; the greatblade's cell core feeds no line; the cut figures are dropped
from the fitting view by display words; the thin-shape test drives booleans,
not `animate`; the keys-only magazine path takes 22 presses.

### Step 4: the rifle (2026-10-13)

Stills (1280x720, default field of view) in `docs/design/combat/step4-*.png`:
`rifle` (from behind), `rifle-side` (`--orbit-yaw 70`), `aim` (frame 128 of
`--hold-aim`, the reticle still wide), `scope`, `fitting`, `card`, `ranger`.

Built: a Rifle row (`weapons.ron` version 6) and two new dials on every row,
`aim_time` (seconds the cone takes to close, nought for the carbine and the
pistol, which read to the bit as before) and `zoom` (1.0 for them). The
`Combatant` keeps `aim_share`, moved by `aim_time` while aim is held, reset by a
swap, reload, dodge and stagger; `Combatant::cone` blends the two ends by it
and `cone_when` keeps them. `advance_bolts` already sweeps; a test fires the
rifle at an actor side on at five ranges and five phases and all 25 land. The
scope (`strata_client::scope`) is entered at a closed aim on a weapon of zoom
over 1.5: the camera at the capsule's eye, the view divided by the zoom, the
mouse slower by it, the local body hidden, a code-made overlay. The item
`veyra:weapon.marksman_rifle` (price 520, Pelagic's board, a Bore signature on
AimSpread), four parts, five schematics, Ranger's Long shots, Wilderness XP.

| damage/s (hit share) | 10 m | 30 m | 60 m | 120 m |
|---|---|---|---|---|
| pistol | 68.2 (1.00) | 43.5 (0.62) | 7.7 (0.19) | out of range |
| carbine | 109.1 (1.00) | 27.9 (0.29) | 4.8 (0.08) | out of range |
| rifle | 54.2 (1.00) | 51.2 (1.00) | 47.5 (1.00) | 40.0 (0.95) |

Rifle row: Single, interval 1.8, bolt 140 m/s, damage 85, posture 55, 4 rounds,
reload 2.4, spread 7.0, aim_spread 0.25, bloom (2.5, 3.0, 0.5, 4.0),
move_spread 3.0, range 260, falloff (150, 260, 0.7), aim_time 0.7, zoom 3.5.

Bands (`balance check`, 34 held): owns-the-long-range 6.21 of the pistol at
60 m (4.0 to 12.0); holds-its-range 0.84 at 120 m of 60 m (0.70 to 1.10);
worst-close 0.79 of the pistol at 10 m (0.30 to 0.85), 0.48 of the carbine;
needs-its-aim 0.01 from the hip (0.00 to 0.40). The hip preset now fires
unaimed (`HoldAndFire { aim: false }`) and `Policy::Aim` waits for a closed
aim before its first pull, so every figure is of a settled shooter.

Ranger: Long shots 1 certifies the rifle (0 XP), 2 aim_time x0.75, 3 reload
x0.85, 4 range x1.15; the novice and the other three branches teach nothing.
Only Ilse Marrow reaches it: Carbine 1 at a marksman trainer, then Routes 1
and the novice at the survey trainer.

Decided for the user and easy to change: every Rifle figure in
`assets/combat/weapons.ron` (interval, spread and aim_time sit on the brief's
limits); the scope's 1.5 threshold and the 52/68 aim view in the client; the
bore signature 1.60 to 0.80 in `marksman_rifle.item.ron`; Frames 3 teaches the
rifle, Fittings 2 the compact magazine, 3 the supported stock, 4 the long
optic and field rest (`trees.ron`); the survey trainer teaches Surveyor and
Ranger (`household/props.rs`); Wilderness XP `wilderness: (rifle: 0.5, long:
85, long_m: 50.0)` in `trees.ron` rates; Long shots 1 costs no XP.

Left rough: the rifle schematic is complexity 3 (the registry refuses 4); two
schematic ids carry a `vigil_` prefix so `first_using` stays on the carbine;
Long shots 4 moves `range` alone (no falloff path in `Weapon::modify`); at
30 m the rifle beats both other guns standing and no band holds it there;
the 10 m band's margin is 0.06 with three dials on their limits; the scope is
at the eye, not the sight bone, and its overlay z (-2) equals the nameplates';
three card words were shortened (Falloff, Full to, Time between shots); the
Ranger sheet's note scrolls under the head at 720p; the scope was checked in
a still and unit tests, not with a live mouse.

### Step 5, first half: what an ability is (2026-10-13)

No stills: nothing is drawn yet (the bar and icons on screen are step 6).
Built: `assets/combat/abilities.ron` (version 1, the sixth table, in the
version and the reproduction test): bar size, statuses `focused` and
`fortified`, eight rows with an effect of Cut, Shot, Strike or Own.
`strata_sim::ability`: `Bar` (a component the sheet requires), `Place`,
`can_slot` (`SlotRefusal`), `BarRequest` in `BarInbox` (refused `InFight` as
the bench switch is), `BarAnswer`, `known_to`, `follow_sheet`, and `can_use`
(`AbilityRefusal`: Empty, WrongWeapon, Cooldown, Stamina, NoRounds, Nothing,
Busy). `CombatIntent.ability` is the edge, `Refusal::Ability` says a refusal
once, a busy press waits out the buffer, `Action::Ability` (outside
`Action::ALL`, no actions.ron row) is the commitment. Registry effects
`Grant` and `Ability`; `check_grants` checks ids and fields. Physics:
`AttackKind::Strike`, bolts that carry the ability's factors.

| id | stamina | cooldown | windup | length | cancel | pace | effect |
|---|---|---|---|---|---|---|---|
| heavy_cut | cut's | 0 | cut's | cut's | cut's | cut's | Cut(Heavy) |
| butt_strike | 20 | 6 | 0.18 | 0.55 | 0.40 | 0.3 | Strike 1.7 m, 50 deg, 8, 70 |
| precision_shot | 25 | 8 | 0.35 | 0.65 | 0.50 | 0.3 | 1 bolt, exact, x2.2, x1.5 |
| aimed_burst | 25 | 10 | 0.20 | 0.75 | 0.65 | 0.4 | 5 bolts, 0.08 s apart, aimed |
| focus | 30 | 18 | 0.15 | 0.30 | 0.30 | 1.0 | Focused 5 s |
| kinetic_absorb | 30 | 20 | 0.10 | 0.30 | 0.30 | 1.0 | Fortified 4 s |
| rapid_reload | 30 | 20 | 0.25 | 0.40 | 0.40 | 0.6 | magazine full |
| shield_recharge | 40 | 30 | 0.40 | 0.60 | 0.60 | 0.5 | shield wait ends |

Presets beside controls (20 bouts, seed 1): precise-pistol 2.62 s to kill
against 3.40 s; burst-carbine 4.30 against 5.32; butt-carbine 1.71 each (never
pressed); absorb-greatblade lowest posture 100 against 88; focus-rifle-hip 4.13
s (its control never kills). Bands (40 held): a-precision-shot-pays 1.24 (1.10
to 1.60, chosen); an-aimed-burst-pays 1.28 (1.15 to 2.00, chosen); butt taken
0.00, breaks 0.00; kinetic-absorb-keeps-the-posture 1.14; focus-puts-the-rifle-
in-its-cone 59.9. Nothing in abilities.ron was tuned. `--abilities`,
`--b-abilities`, `PressAbilities` (a component, not a Policy); a cut ability
counts under `cuts`, not `Abilities`.

Decided for the user and easy to change: every figure above (`abilities.ron`);
keys Q, E, T (a class's places) and 3, 4 (utility), `ABILITY_KEYS` in the
client's `combat.rs`; the heavy cut left Q's own key for place 0 (`blade.cuts.3`
also grants it); a newly known ability takes the first empty place of each
class it works with, a cleared place stays cleared, a forgotten one leaves all;
the bar changes anywhere out of a fight.

Left rough: `advance_bolts` falls back to a ray cast when Parry's sphere cast
misses a bolt flying dead down a standing capsule (the Exact cone's common
case); the plain pistol went from 3.43 s to 3.40 s and
pistol-matches-the-carbine-at-mid-range reads 1.58 against a ceiling of 1.60.
The butt strike bands measure a fight where it is never pressed; shield
recharge looks weak and has no band; a broken guard can still stagger a
Fortified bearer; `Bar.granted` is a still-only backdoor saved with the bar.

### Step 5, second half: the other eleven (2026-10-13)

No stills. `abilities.ron` is version 2: 19 rows, 6 statuses, `hit_arc`. Names: `StatusRow.{pace, shield_locked, disarmed,
reflects}`, `Shot.{on_hit, jump, blast, breach}`, `ShotCone::Hip`,
`Strike.{scaled, on_hit}`, `Hit`, `Move` (`Along`, `ThenStrike`),
`AbilityRow.armed`, `ShotEffects`, the pure `ability::splash`. Where read:
a status and a breach in `Combatant::hit` after the guard (only Open lands
a status); jump and blast in `splash`, which the physics asks; Reflecting
where a guarded bolt is despawned and at a blocked barrel; Slowed in the one
`speed_scale` (it takes the sprint, as any pace under 1 does); Shield
broken in the shield's recovery (its own `shield_wait`); Disarmed in
`begin`, the trigger, a volley's owed rounds and `can_use`. Statuses: Slowed 0.6, Shield broken, Disarmed, Reflecting 1.0.

| Ability | Class | Stam | Cool | Wind | Len | Pace | Effect |
|---|---|---|---|---|---|---|---|
| ricochet | Pistol | 20 | 9 | 0.15 | 0.45 | 0.6 | jump 8 m at 0.8 |
| disarm | Pistol, Blade | 25 | 14 | 0.20 | 0.55 | 0.5 | Hit 0.6/0.5, Disarmed 2.5 s |
| suppressing_fire | Carbine | 30 | 14 | 0.15 | 1.10 | 0.3 | 10 hip rounds, Slowed 3 s |
| explosive_shot | Carbine | 35 | 16 | 0.40 | 0.80 | 0.4 | blast 4 m, 12 / 60 |
| charged_shot | Rifle | 35 | 14 | 0.90 | 1.20 | 0.2 | Exact 1.4/1.5, breach |
| armour_break | Rifle, Greatblade | 30 | 18 | 0.30 | 0.70 | 0.4 | Hit 0.8/1.0, Shield broken 8 s |
| thrust | Blade, Glaive | 25 | 8 | 0.15 | 0.60 | 0 | Move 4 m, Strike 1.3/1.2 |
| reflect_shot | Blade | 30 | 18 | 0.10 | 0.30 | 1.0 | Reflecting 3 s |
| sweeping_cut | Greatblade, Glaive | 35 | 10 | 0.35 | 0.85 | 0.2 | Strike arc 110 |
| vault | Glaive | 30 | 12 | 0.10 | 0.75 | 0 | Move 7 m, Strike arc 60 |
| combat_slide | any (utility, armed) | 20 | 6 | 0 | 0.50 | 0 | Move 6 m along Input |

All eleven keep the design's figures. Presets, each beside its control (60
bouts): `suppress-carbine-vs-rusher` 1.71 s against 1.60, taken 0 both;
`disarm-pistol-vs-rusher` 1.88 against 1.76, taken 0 both;
`charged-rifle-vs-dummy-60m` 1.77 s against 2.95 (1.67 of the dealt a
second); `break-greatblade-vs-rusher` posture low 28.6 against 42.2;
`sweep-glaive-vs-rusher` -10 against 24; `reflect-blade-vs-carbine-25m`
returns 12 of 140, dead at 3.53 s against 3.69. New metric `FirstCut` (the
bout second of the first cut landed, else the bout's length) and the
`*-vs-carbine-6m` presets: thrust 0.85 s and 1.2 cuts, vault 1.01 s and
1.05, the plain blade and glaive none in 1.12 s. 58 bands hold; the new
ones are measured, not chosen.

Melee against a gun from 25 m: still no (dead at 2.13 s, 11 m out); from
6 m the closers land a cut and still lose. They cross a gap, not a field.

Decided for the user and easy to change: every figure above in
`assets/combat/abilities.ron`; the charged shot's charge is its 0.9 s windup;
the harness never presses the slide; `CLOSING_RANGE` 6 m in `tools/balance/src/lib.rs`.

Left rough: the two chosen bands asked for (suppressing fire at 0.90 and
disarm at 0.92 of the plain fight's damage taken) could not be built, the
plain carbine and pistol taking 0 from the rusher from every start down to
0.8 m, against two or three rushers and with a pool of 400; they ship
unchosen as "takes 0". Melee across 25 m needs longer moves, a sprint or
cover. Ricochet, explosive shot and the slide have no preset; reflect shot
costs the bearer 0.16 s of life. Slowed on an opponent is tested without the AI.

### Step 5, third part: melee reaches a gun (2026-10-13)

Why: the second half measured a blade or glaive with its closer winning 0.00 against a carbine from 25 m and from 6 m. Built, in plain words:

- **Caught** (`melee.ron` v3, `caught: (posture 1.6, damage 1.0)`): a cut made with a melee weapon that meets no guard on a bearer holding a gun is multiplied, once, in `Combatant::hit` (`is_caught_by`). A gun's butt strike never; a melee bearer and any bolt read as before to the bit. The rusher's heavy on a gun-bearing player is caught too (40 posture becomes 64).
- **Cover** (`Move.cover: Some((arc, share))`, `abilities.ron` v3): while the movement runs, a bolt from within `arc` of it is Blocked at `share` of its damage, all its posture, no stamina; Reflecting returns it. Thrust is Move(6.0, 0.28), vault Move(10.0, 0.55), both cover (60, 0.8).
- **Instant** (`reflect_shot`: windup, length, cancel 0, 4.0 s): an `Own` ability of length 0 pays, cools down and applies in the press tick and commits the actor to nothing, so the guard stays up. The check refuses any other effect of length 0.
- **Harness**: `sprint` on `CloseAndCut` and `GuardAndCut`; `PressAbilities` presses a covered move from up to 1.5 times its range after a hit in the last half second, and a side with a covered move raises reflect shot at the first bolt as a guard does (the kit cannot use it otherwise, it never aims). Presets: the four `*-2m` (plain `cutting` policies), `sprint-blade-vs-carbine-25m`, `kit-{blade,glaive,greatblade}-vs-carbine-{25m,12m}`, `kit-blade-vs-pistol-25m`. Tests: caught, cover, instant, the checks, the rusher's heavy, sprint for both policies.

Measured at 60 bouts, seeds 1 and 1000 alike (won, seconds, taken, dealt). No dial was kept: every preset reads the same before and after tuning.

| preset | won | s | taken | dealt |
|---|---|---|---|---|
| blade-vs-carbine-2m | 0.00 | 1.41 | 140 | 91 |
| greatblade-vs-carbine-2m | 0.00 | 1.12 | 140 | 0 |
| glaive-vs-carbine-2m | 0.00 | 1.13 | 140 | 17 |
| blade-vs-pistol-2m | 1.00 | 1.80 | 72 | 140 |
| sprint-blade-vs-carbine-25m | 0.00 | 1.83 | 140 | 0 |
| kit-blade / glaive / greatblade 25m | 0.00 | 2.29 / 2.48 / 1.83 | 140 | 55 / 43 / 0 |
| kit-blade / glaive / greatblade 12m | 0.00 | 1.59 / 1.67 / 1.15 | 140 | 0 / 16 / 0 |
| kit-blade-vs-pistol-25m | 0.00 | 2.36 | 140 | 20 |

Bands: `open-ground-is-the-guns` (chosen) holds, 0.00 against at most 0.25. **As first built, `a-blade-in-reach-beats-a-gun` (0.70 to 1.0) read 0.00 and `melee-can-close` (0.25 to 0.65) read 0.00; the first is met by the caught stagger below, the second is still unmet.** In reach the first burst lands before the first cut; the second cut breaks the carbine at 0.65 s but the 0.42 s stagger does not stop its second burst. Inside the brief's limits the best is 0.03 (caught.damage 1.4, which breaks `a-made-edge-shows` at 1.48 because the dummy holds a gun). Older bands moved by caught (all measured, none chosen, limits rewritten): hooked edge posture 1.21 to 1.37 (ceiling now 1.60); thrust cuts landed 1.20 to 2.70 (limits 2.0 to 3.5) and first cut 0.85 to 0.90; vault cuts landed 1.05 to 3.30 (2.5 to 4.0) and first cut 1.01 to 1.12; reflect-shot dealt 12 to 24 to 29 (ceiling 45). Every other older band is unchanged, pistol-matches-the-carbine at 1.58 against 1.60.

Decided for the user and easy to change: `caught` 1.6 / 1.0 in `melee.ron`; cover arc 60 and share 0.8, thrust 6 m in 0.28 s and vault 10 m in 0.55 s in `abilities.ron`; a gun bearer is caught by any blade including the rusher's; a closer blocks most fire from ahead; reflect shot is instant. Levers outside the limits, measured at 40 bouts and for the user to choose: `caught.posture` 2.94 and up (3.0: blade-2m 1.00, thrust-blade-6m 1.00, glaive-2m 0.43, greatblade-2m 0.00; a light alone then breaks a gunner and the rusher's one heavy staggers a gun-bearing player), or a longer stagger on a caught break (0.52 s reads 0.725, 0.62 s 0.875; measured on the global Stagger row, a caught-only factor in `melee.ron` would be a new rule). Neither moves `melee-can-close`: no kit begins a cut, a sprinting blade needs about 2.9 s for 25 m and the carbine kills in 1.83 s. That needs cover for most of the approach (a guard with reflect shot walked in, or a far longer covered move).

Left rough: the book counts an instant reflect shot nowhere (it counts entries into `Action::Ability`), and the HUD needs its own cue for it (step 6); the 1.5 times press rule spends a covered move where its strike cannot land (kit-blade-12m deals 0); `GuardAndCut`'s guard-down-while-sprinting is exercised by a first-cut test only; the rusher's heavy being caught is a change to what a player with a gun out feels.

#### After the review: the caught stagger (2026-10-13)

The review found the in-reach band one rule away: the blade's second cut broke the carbine and the 0.42 s stagger ended before the next cuts landed. `caught` gained a third factor, `stagger: 1.5` (`melee.ron` v4): the stagger a caught cut begins lasts that many times the row's, held on the `Combatant` for that stagger alone, so a bolt's break, a butt strike's and a melee bearer's read as before. Measured at 60 bouts:

| caught stagger | blade-vs-carbine-2m | thrust-blade-vs-carbine-6m |
|---|---|---|
| 1.0 | 0.00 | 0.00 |
| 1.25 | 0.80 | 0.17 |
| 1.5 (shipped) | 0.92, taking 100 of 140 | 0.82 |
| 1.75 | 1.00 | 0.82 |

`a-blade-in-reach-beats-a-gun` is now chosen and holds, and `a-thrust-carries-the-blade-in` (the thrust from 6 m wins at least 0.55) is chosen with it. The rusher's heavy on a gun-bearing player still needs two to break (64 posture each), and that stagger is now 0.63 s.

What the same probe showed, and what it means:

- The greatblade and the glaive lose in reach for a different reason than the table above said: the firing policy backs away at 0.7 of its pace inside 7 m, and a slow cut roots its user, so the cut never reaches. Traced on the sweep itself: the geometry is right, the target has left. A slow class needs a move that carries it (vault, a sprinting cut), not a bigger number.
- `melee-can-close` stays unmet and unchosen: across 25 m a player's carbine deals 140 in 1.8 s and no kit lives through it. That fight is the mirror, a player's own carbine in a policy's hands at 110 damage a second, and no opponent in this game fires like it: the sentry fires one bolt a charge at half damage. Whether melee is a first order choice is therefore asked of the game's own opponents in step 8 (the marksman, the duelist, the sentry and the rusher at the encounter's distances), where each melee class gets a band beside each gun.
- If the mirror itself should be winnable, the lever the numbers point at is the guard against bolts: a blade's guard pays 20 stamina a carbine bolt (share 0.60 at 1.40 a point), five bolts to a broken guard. A bolt row of its own on each class's guard (most of the damage off, a third of the stamina) would let a guarded walk reach thrust range. Not built: it would also make every sentry harmless to a raised guard, which is a decision about the whole game.

### Step 6: the bar, the statuses and the loadout (2026-10-13)

Stills in `docs/design/combat/step6-*.png`: `bar` (carbine, a cooldown
shown), `bar-melee` (greatblade, Fortified above the vitals), `refused` (red
frame and "On cooldown, 5 s"), `loadout` (Carbine E in focus, KNOWN filtered,
reasons shown), `box` (the Blade tree's Cuts 3 with the heavy cut's square),
`bar-4k`.

Built: `strata_registry::icons` (`IconCatalogue`, loaded into
`GameRegistries.icons`, serde_json appended to the workspace); `ui::icons`
(class symbols, `EMPTY_PLACE`, `STAGGERED`, `IconHandles`, the `strata:icon`
widget); `action_bar` (`bar_view`, `status_view`, the `strata:bar` layer,
`BarFeedback`); status squares on the nameplates; `ui::sheet::loadout` (the
tab, `LoadoutState`, `answer_choices`, `answer_sim`, `follow_focus`);
`AbilityRow::facts` in the sim; a granting box's square; `--open-loadout`.
A press on a place is no longer swallowed: `follow_focus` waits out a
pointer press, with a ui test that presses, waits a frame and releases.
Export files are chosen by size times the UI scale.

Sizes: squares 56 px, weapon marks 28, player statuses 28, plate statuses 18,
card square 128, KNOWN rows 40, grant square 32. Scope z: vignette -4,
plates -3, markers and floating numbers -2, scope -1, HUD 0.

Decided for the user and easy to change:
- Sizes above; a draining overlay, not a radial sweep.
- Greatblade and glaive share the blade's symbol until the library has marks for them (the heavy-tool mark read as a gun at 28 px).
- The slot chips left the vitals; the bar's marks replace them.
- Refusal words: "On cooldown, N s", "Not enough stamina", "Not with this
  weapon", "Magazine too low", "Nothing to do", "Disarmed".
- Busy dims the squares as Held, as briefed.
- Floating numbers now draw under the HUD layers.

Left rough:
- heavy_cut gives no accent pulse and its card has no figures (a cut's
  numbers are the action's row).
- A status's share divides by the longest time any row gives it.
- Plate status squares do not fade with the plate between 45 and 60 m.
- Every focus move rebuilds the sheet; KNOWN's scroll resets, not stressed.
- The tab does not caption which place is being filled; the trees' legend
  still shows under it.
- Dead `Vitals.slots`, `SlotChip` and SLOT ids remain in combat_hud.
- The card's last figure scrolls at 720 px; the grant box's cue.

### Step 7: the trees teach (2026-10-13)

`trees.ron` changes in names, descriptions and effects only: no id, cost, XP
gate or prerequisite. All nineteen abilities are granted (a shipped-files test
holds it). Blade reads One hand (`blade.cuts`), Guard, Greatblade
(`blade.breaking`), Glaive (`blade.footwork`); the heavy cut is the novice's;
the design's `Ability(heavy_cut, posture)` is `Action(Heavy, posture, Mul 1.15)`
on One hand 3 (a Cut takes its figures from the action's row). Guard effect:
`Guard { field, op, value }` folds into `EffectiveTables.guards`, read only by
`Combatant::guard_row`; an unfitted actor reads the table to the bit. The
card's "Taught by" is `ability::taught_by` (a learned box, else "Given by
hand", else the first box).

| Box | Gives |
|---|---|
| marksman.novice | Pistol; butt_strike |
| carbine 1 / 2 / 3 / 4 | Carbine, aimed_burst / reload x0.85, suppressing_fire / interval x0.85, explosive_shot / damage x1.1, aimed_burst cooldown x0.8 |
| pistol 1 / 2 / 3 / 4 | reload x0.8, precision_shot / magazine +4, disarm / interval x0.85, ricochet / damage x1.15, precision_shot cooldown x0.8 |
| steadiness 1 / 2 / 3 / 4 | swap x0.8, rapid_reload / aim speed 0.8 / bolt speed x1.1, butt_strike posture x1.2 / aim speed 0.9 |
| field craft 1 / 2 / 3 / 4 | range x1.25, shield_recharge / reload x0.85 / shield regen x1.5, combat_slide / sprint +0.5, combat_slide cooldown x0.75 |
| marksman.master | magazine x1.25, hurt_wait -0.5, rapid_reload cooldown x0.7 |
| blade.novice | Light, Heavy, Blade; heavy_cut |
| One hand 1 / 2 / 3 / 4 | Followup, thrust / Followup +6, disarm / reflect_shot, Heavy posture x1.15 / Heavy damage x1.15, thrust cooldown x0.8 |
| Guard 1 / 2 / 3 / 4 | posture_regen 48, guard stamina x0.9 / hurt_wait -0.5, parry window +0.03 s / posture +20, guard share +0.05 / stagger x0.7, riposte x1.25 |
| Glaive 1 / 2 / 3 / 4 | Dodge -4 stamina, Glaive, thrust / dodge cancel 0.26, sweeping_cut / dodge travel +0.6, vault / dodge contact_end 0.28, vault cooldown x0.8 |
| Greatblade 1 / 2 / 3 / 4 | Light posture +6, Greatblade / Followup posture +8, sweeping_cut / heavy windup -0.04, kinetic_absorb, armour_break / Heavy posture 110, kinetic_absorb lasts +2 s |
| blade.master | unchanged |
| Long shots 1 / 2 / 3 / 4 | Rifle, focus / aim_time x0.75, charged_shot / reload x0.85, armour_break / range x1.15, charged_shot windup x0.8 |

Bars: the arrival has heavy_cut on Blade, Greatblade, Glaive, butt_strike and
aimed_burst on Carbine, butt_strike on Pistol and Rifle. Tovan Reyes: Blade
[heavy_cut, disarm, thrust], Greatblade [heavy_cut], Glaive [heavy_cut, thrust],
Carbine [butt_strike, aimed_burst, suppressing_fire], Pistol [butt_strike,
disarm], Rifle [butt_strike]. Ilse Marrow: Pistol [butt_strike, precision_shot,
disarm], Carbine and Rifle [butt_strike], Blade [disarm]. Dr. Faal: nothing. An
older member (Cuts 1 to 3, Breaking 1, Pistol 1) loads unchanged, still cuts
heavily, has thrust, disarm, precision_shot placed; reflect_shot waits. The
fixture is today's serialisation minus the bar field, not an old file's text.

Harness: all 73 bands hold, no figure moved (pistol-matches-the-carbine 1.58 of
1.60); the first hour passes. No band sees what changed: the harness rusher is
unfitted (heavy posture 80, the game's now 92) and no preset guards with a Guard
box; `cuts-3-lands-the-heavy` passes on the novice alone (rationale reworded).
Unmeasured: heavy posture 126.5 on a Blade (was 110), 208.7 on a Greatblade (was
181.5); Guard 4 gives a Greatblade riposte 112.5, over the actor's 100.

Stills `docs/design/combat/step7-*.png`: marksman, loadout (flags); blade, ranger,
novice, guard (click, scroll, `scratch/combat/s7scroll.sh`); bar; box (Greatblade 3
grants two: the card shows the first's square, the second is in the effect lines).
Decided for the user, easy to change: which box grants what; the heavy at the
novice box; branch names over kept ids; the four Guard figures (stamina x0.9,
parry +0.03 s, share +0.05, riposte x1.25). Left rough: step 8 wants a rusher on
`opponents.ron`'s sheet and a guarding side with Guard 1 to 4; the loadout's
Glaive row and UTILITY are below the fold at 720 p.

### Step 8, first half: a marksman and a duelist (2026-10-13)

Stills `docs/design/combat/step8-*.png` (frames 284, 294, 282, 124, 334; they
move with the frame rate): marksman aim, lock, scope (found centre at 3.5x, beam
end on), duelist guard (`--hostiles duelist@6`), parry (`@5 --press 0@322`).

Both are kinds of `OpponentBrain` (`OpponentKind`; `drive` takes a `Seen`); the
sentry and rusher replay to the bit. The marksman holds 40 to 80 m, flees inside
12; its 1.6 s charge follows the target and holds the last 0.35 s, so crossing
its line then is missed, standing is hit (sim test, 30 s at 60 m). Game and
harness both fire its own 110 m/s (a game test holds it). The duelist walks in
unguarded past 7 m, guards inside, attacks inside 1.9 m (light, follow up,
light, follow up, heavy), is open while it swings and 0.5 s after. Seeing a
Heavy begin inside 2.85 m it lowers the guard until 0.22 s in, raises it and
does not swing meanwhile: a heavy is Parried, a light Blocked. An attack counts
when the sim takes it (`Combatant::can_begin`), so a duelist short of stamina or
Disarmed keeps its guard. Its `parry_rest` 0 applies to that raise only.

Rows, `assets/combat/opponents.ron` v3. marksman: bolt_speed 110, charge 1.6,
lock 0.35, recover 2.8, cooldown 1.5, stagger_cooldown 1.2, walk_speed 2.2, hold
40 to 80, fire_range 120, flee 12, pace 0.7. duelist: walk_speed 3, reach 1.9,
cooldown 1.6, initial_cooldown 0.6, wary 7, react 0.22, heavy_every 3, rest 0.5,
parry_rest 0, stagger_cooldown 0.6, slow_margin 0.1, read_reach 1.5. The harness
fits both from their sheets as the game does (sentry and rusher unfitted).

Measured, 60 bouts, release, no band chosen. Opponent: marksman shots, hits;
duelist blocks, parries, breaks. Triples are light, heavy, guard.

| preset | win | s | dealt | taken | shots/hits, cuts/landed | opponent |
|---|---|---|---|---|---|---|
| pistol, carbine, rifle vs marksman 60m | 0.85, 0.33, 1 | 15.3, 21.9, 3.0 | 113, 99, 105 | 131, 184, 0 | 41/8, 92/7.6, 2/2 | 3.3/3.1, 4.5/4.5, 0 |
| carbine-close-vs-marksman-60m | 1.00 | 8.0 | 106 | 13 | 14.5/5.0 | 1.6, 0.3 |
| blade-close-vs-marksman-60m | 1.00 | 9.3 | 105 | 9 | cuts 2/2 | 1.0, 0.2 |
| pistol, carbine, rifle vs duelist 15m | 1 | 1.7, 1.4, 3.4 | 105 | 0 | 5/5, 6.5/5, 2.7/2 | 0, 0, 0.3 blocks |
| blade triple | 1, 0, 1 | 2.4, 11.3, 30.9 | 105, 55, 126 | 31, 140, 47 | 4/4, 6/1, 11/10 | 0/0/0, 0/5/0, 4/1/5 |
| greatblade triple | 1 | 4.0, 5.5, 5.7 | 105 | 62, 14, 9 | 4/3, 3/3, 3/3 | 0/1/0, 1/0/2, 1/0/0 |
| glaive triple | 1 | 3.4, 4.0, 21.8 | 105, 105, 125 | 31, 45, 88 | 5/5, 3/3, 5/5 | 2/0/1, 1/0/0, 0/0/2 |

Blows to break its guard: a carbine or pistol round the 5th, a rifle round the
2nd, a Blade light the 5th (4th in a light and follow up chain). Presets that
end before the opponent acts, so no band should sit on them as they stand:
`rifle-vs-marksman-60m`, the three `*-vs-duelist-15m` (dead before 7 m) and the
light presets.

Decided for the user and easy to change: every figure above in `opponents.ron`;
the default encounter's one marksman at 60 m behind the start, on the one
engagement rule (a lone one never wakes, so stills stand a sentry at 9 m beside
it); the beam (`strata_client::combat_beam`, 1.5 px faint, 2.5 locked, via
`MarksmanTable::locked`); plates to 100 m; `--orbit-pitch` for stills; the
duelist's `parry_rest`, since melee.ron's 0.5 s could only ever Block.

Through the scope the beam points at the eye and is a dot, so the lock has a
second cue that reads end on: the glow at the marksman's muzzle grows with the
charge and flares 2.5 times in the lock (`LOCK_FLARE`, `glow_size` in the
client's `combat.rs`; the scope still is that frame).

Left rough: the lock's flare is a flat square; the duelist
closes to 1.9 m, bodies overlap, it never steps back; a Greatblade heavy is
mostly Blocked, not Parried; the marksman fires into a hill as the sentry does.
Where 60 m ahead has no clear line, the default encounter waits five seconds
for its marksman once the other three can stand, then begins without it
(`FAR_WAIT_FRAMES` in `apps/game/src/combat.rs`).

### Step 8, second half: tuned, and what each thing is worth (2026-10-13)

`balance worth` writes `docs/balance/combat-worth.md` (60 bouts, about four
minutes) with the log `assets/balance/combat_tuning.ron`. `balance check`: 122
bands, every chosen one holds with every older band. Versions: weapons 7,
opponents 4, abilities 4. A riposte is the guard row's figure, unscaled by the
swinger's `swing_posture` (`Combatant::riposted`, sim test
`a_riposte_is_the_guard_rows_figure_whatever_the_swingers_class`). The game's
rusher drops blade.cuts.3 (Heavy posture 80 as before step 7, not 92); the
harness fits all four opponent sides from their sheets, and every band held.

**Chosen bands, 20 bouts.** Standing carbine against the marksman 0.00 (<=0.15);
rifle hand 1.00 (>=0.60), marksman firing 1.0 a bout; carbine closing 1.00; the
three classes clear the sentry 1.00 taking 12, 24, 2.4 (<=70); kit blade 1.55
times the carbine's seconds (1.1 to 2.5); each class clears the rusher 1.00, the
blade taking 27.5 (<=41.25); blade reaches the marksman 1.00; heavy-only blade
against the duelist 0.00 (<=0.35), guarding blade 1.00; guard boxes 0.77 of
untrained (<=0.90); pistol 0.90 at 30 m against the rifle hand (>=0.50), rifle
1.00 at 80 m (>=0.80); parry share of the rusher's heavies 0.26 (0.15 to 0.60,
0.17 to 0.26 over seeds, near its floor; the old form was vacuous).

**Tuning log:** marksman recover 2.8 to 2.2, cooldown 1.5
to 0.9, walk_speed 2.2 to 1.8; duelist rest 0.5 to 0.3 (the guarding blade won in
30.9 s, past the cap; five other melee fights against it roughly doubled);
Rifle aim_time 0.7 to 0.875; disarm 2.5 s to 1.5 s; charged shot 1.4 to 1.1;
explosive shot bolt 1.0 to 1.6, blast 12 to 24, windup, length and cancel about
halved. The rifle hand (`StopAndShoot`) was walk 1.8, stand 0.6
(one shot in 3.3 s, set to give the pistol its middle); it is now walk and stand
0.9, the aim's settling time. The pistol wins 0.87 at 60 bouts (0.52 at aim_time
0.7). The riposte fix moved greatblade light against the duelist 4.0 to 4.4 s.

**What the page says.** On the dummy the carbine leads at 10 m (110.5), the rifle
at 30, 60, 120 m (48.0, 44.7, 37.6); the pistol leads nowhere but is fastest on
the sentry (2.2 s). Melee is **not** a first order choice against the game's
opponents: a gun beats every class on seconds and damage taken against all four;
the rusher lands one Heavy (27.5) on each class and none on a gun, the duelist
costs melee 45 to 72 against 0; nearest is the glaive on the sentry (3.5 s, 2.4
taken). The lever not built: a guard a melee opponent holds at range that costs
it less against bolts, or plating bolts do less to than cuts.

**Abilities.** In band: heavy cut 1.16, precision shot 1.24, aimed burst 1.28,
ricochet 1.11, suppressing fire 1.23, charged shot 1.24, disarm 0.90 (the fight
goes 5.0 s to 9.2 s), reflect shot and sweeping cut 1.05, thrust 0.00 to 0.62. Chosen false, dead
or unproven: rapid reload (1.02 to 1.10 over seeds), explosive shot (1.03 to
1.08), kinetic absorb (no change), butt strike and vault (no ratio), focus on a
standing rifle (0.91; +98% only over one that strafes). Over: armour break
against the duelist (8.3 s to 2.6 s), shield recharge, combat slide.

**Decided for the user and easy to change.** Focus at 5 s (3 s reads 1.50 but
moves the older focus band 59.9 to 6.0); melee as a niche or the unbuilt lever;
vault as is or twice the strike; Rifle aim_time 0.875 or damage 0.8 times.

**Left rough.** Pure melee presets are seed independent (a win share is 0 or 1; step 9 seeded them).
The greatblade parries nothing in any preset (riposte 112.5 and Heavy 208.7 are
unmeasured). Older `pistol-matches-the-carbine-at-mid-range` reads 1.58, 1.66,
1.47, 1.24 over four seeds and fails at one; its limits could not move here.

### Step 9, first part: a cutting hand has a seed (2026-10-13)

**What the hand draws.** A cutting policy (`CloseAndCut`, `GuardAndCut`, and
the abilities a melee side presses) draws four figures from the driver's
generator, after every draw a gun's driver makes: `ARRIVAL` (0 to 0.25 s,
once, on first reaching the target), `PAUSE_SPREAD` (0.85 to 1.35 on each
pause), `STEP_SPREAD` (0.9 to 1.1 on the reach it closes to, never past the
weapon's real reach, so a draw over one reads as one) and `CHAIN_LAG` (0 to
0.08 s, on a Light's return cut and on every opening, so a pause of nought
still has it). A press of a cut, strike or hit waits on the hand's one clock
(`Driver::hand_free`) and sets its next pause. The opponents draw nothing and
nothing under `crates/` moved. Every band with no cutting side reads the same
figures to the digit.

**How much of a distribution.** The first attempt drew the arrival alone,
and at 60 ticks a second that is 16 values, so `blade-vs-rusher` showed 16
distinct bouts in 60 and in 240: one fight shifted, not a distribution (16
was its ceiling). The abilities pressed on arrival were one trajectory
outright. Distinct (time, damage taken, winner) in 60 bouts from seed 1 now:
`blade-vs-rusher` 17 (3.10 to 3.37 s), `worth-blade-vs-rusher` 9,
`worth-greatblade-vs-rusher` 2, `worth-glaive-vs-rusher` 4,
`worth-greatblade-vs-duelist` 4 (3.78 to 5.73 s), `worth-glaive-vs-duelist` 4,
`worth-blade-vs-sentry` 3, `worth-glaive-vs-sentry` 5,
`disarm-blade-vs-duelist` 22 (2.58 to 9.17 s),
`break-greatblade-light-vs-duelist` 20 (2.70 to 6.15 s),
`sweep-glaive-vs-rushers` 15 (8.18 to 9.73 s), `blade-vs-carbine-2m` 29.
`worth-blade-vs-duelist` is still one bout (4.13 s, taking 62.0 every time):
the blade thrusts in from a sprint and its three cuts then wait on stamina,
so the stamina sets every opening and swallows a lag of 0 to 0.08 s. That is
a property of the fight and the constants were not widened for it.
`blade-heavy-vs-duelist` has 16 distinct bouts and wins none.

**What moved against the old page.** Taken as a distribution, a blade in
reach of a carbine (`blade-vs-carbine-2m`) wins 0.73 over four 60 bout
windows (0.58 to 0.82), not 0.92. Disarm took 0.90 of its control's damage as
one trajectory at a rest of 0.3, and reads 1.12 (1.07 to 1.21) as a
distribution at 0.5: it does not cut the damage taken. Armour break read
0.32 then, +216% on the page, and reads 0.48 (0.46 to 0.54), +110% on the page:
still the answer to a guard, no longer three times. The sweeping cut reads 0.98
(0.96 to 0.99) against its control, not 1.05. The master wins 0.46 of its
bouts against the novice (0.38 to 0.52), not all. The worth page's melee rows
against the rusher and the duelist moved by tenths of a second; what each
melee class takes from the rusher is still one blow, 27.5.

**Bands redrawn or unchosen.** Readings are seed 1, 101, 501 at 20 bouts and
the 60 bout window from seed 1 (and 101, 501, 1001 at 60 where said).

| Band | Old limits and figure | Readings | Now |
|---|---|---|---|
| `a-blade-in-reach-beats-a-gun` | 0.70 to 1.00, 0.92 | 0.75, 0.70, 0.80; 60 bouts 0.82, 0.58, 0.73, 0.78 | 0.45 to 1.00 at 60 bouts |
| `a-thrust-carries-the-blade-in` | 0.55 to 1.00, 0.82 | 0.50, 0.55, 0.75; 60 bouts 0.53, 0.48, 0.50, 0.58 | 0.30 to 1.00 at 60 bouts |
| `patience-beats-the-duelist` | 0.6 to 1.0 inside 30 s | 20 bouts inside 45 s 0.90, 0.95, 0.80; 60 bouts 0.88, 0.98, 0.88, 0.88 | same limits, new `cap: 45` |
| `a-parry-beats-a-block` | 0.0 to 0.9, 0.65 | 20 bouts 0.22 to 1.10; 60 bouts 0.43, 0.29, 0.42, 0.42 | same limits at 60 bouts |
| `ability-armour-break` | 0.25 to 0.40 unchosen, 0.32 | 0.45, 0.55, 0.44; 60 bouts 0.48, 0.51, 0.46, 0.54 | 0.35 to 0.65 unchosen, measured; wanted 0.625 to 0.97 |
| `ability-disarm` | 0.625 to 0.97 chosen, 0.90 | 1.16, 1.09, 1.15; 60 bouts 1.12, 1.18, 1.21, 1.07 | unchosen, measured as a distribution |
| `ability-sweeping-cut` | 1.03 to 1.60 chosen, 1.05 | 0.98, 0.96, 0.98; 60 bouts 0.98, 0.96, 0.98, 0.99 | unchosen, measured as a distribution |
| `ability-reflect-shot` | 1.03 to 1.60 chosen, held at seed 1 | 1.03 on the floor at every seed: 1.027 to 1.039 over six 60 bout windows | unchosen, measured |
| `blade-master-beats-novice` | 0.80 to 1.00 chosen, 1.00 | 0.45, 0.65, 0.35; 60 bouts 0.52, 0.45, 0.38, 0.50 | unchosen, 0.46 |

A band may now carry `bouts` and `cap`. A chosen gun band
(`focus-puts-the-rifle-in-its-cone`, `ability-focus`, `ability-shield-recharge`,
`ability-combat-slide`) reads outside its limits at one of the other seeds at
20 bouts; they have no cutting side, were not touched, and are not this step's.

**The duelist's rest** stays 0.5 (opponents.ron version 5). The rule moves
it only if the guarding blade wins under 0.6 of its bouts inside 45 s at 0.5
and at 0.3; with the cutting hand using `STEP_SPREAD` it wins 0.88 inside 45 s
at 0.5 (0.68 inside 30 s) and 1.00 at 0.3 (0.93 inside 30 s), 60 bouts each.

**An experiment, not shipped.** With an opening lag of 0 to 0.25 s in place
of 0 to 0.08 s, `blade-vs-carbine-2m` wins 0.50 of 60 bouts (0.82 with the
shipped hand, 0.92 with the old hand that drew nothing). The lag stays at
0 to 0.08 s because the game buffers a press, so a practised hand chains
cuts within a few ticks.

### Step 9, second part: the bulwark (2026-10-14)

Stills: `docs/design/combat/step9-bulwark-{advance,blocked,open,blade,default}.png`
(dense `--shot-frames` runs; frames 98, 131, 120, 124; default 70).

**The one new rule.** `GuardRow.bolts: Option<BoltGuard>` (share, stamina,
posture) is what a guard reads against a bolt where set; no class row sets it,
so every player and older opponent reads as before. A blast is now met by no
guard (`Blow::blast`, no direction, Open): before, a blast's blow came from the
contact point and a guard facing it blocked or parried the row's share. A
guarding player hit by a blast now takes it whole even if the bolt itself was
blocked. No opponent fires one today; held by a sim and a physics test.

**The bulwark.** A fifth kind of the one brain. Guard up and facing the target,
it walks in at `advance_speed` 4.6 (the motor speed is divided by the aim pace,
`OpponentKind::motor_speed`, so the guard does not slow it), and inside reach
with its cooldown run out lowers the guard and cuts a light and its follow up,
open from the cut until `rest` after. Idle (before the encounter engages) it
holds the guard up and faces (`OpponentBrain::idle`; the other kinds do
nothing, so their replays hold). A guard with `parry: 0` no longer parries on
the tick it comes up. Row (opponents.ron v6): advance 4.6, reach 2.0, cooldown
1.8, initial 0.4, rest 0.45, guard share 0.60, arc 75, stamina 1.40, posture
0.40, bolts (0.97, 0.15, 0.0). Harness and game both fit it from its sheet and
`with_guard`; the game carries `[Blade, None]`, the harness a holstered second.
The marksman and duelist replay hashes were recorded before the brain was touched.

| Preset (15 m, seed 1, 60 bouts) | win | s | taken |
|---|---|---|---|
| standing-carbine | 1.00 | 6.65 | 18.9 |
| punish pistol / carbine / rifle | 1.00 | 9.85 / 5.35 / 7.35 | 25.0 / 11.9 / 11.9 |
| blast-carbine | 1.00 | 5.27 | 11.9 |
| worth blade / greatblade / glaive | 1.00 | 4.12 / 5.70 / 3.27 | 45.0 / 75.5 / 37.8 |

**Chosen bands, at seeds 1, 101, 501.** `a-gun-can-punish-the-bulwark` 1.00 at
all; `the-bulwark-costs-a-gun` 3.3, 3.2, 3.2 times the rusher's seconds (floor
2.0); `a-blade-answers-the-bulwark` 1.00; `melee-is-quicker-on-the-bulwark` 0.77
(glaive 0.61, greatblade 1.07); `holding-fire-into-the-bulwark-costs-time` (new)
1.24, 1.26, 1.21. Not met, `chosen: false`: `fire-into-the-bulwark-is-wasted`
(standing carbine wins 1.00: a carbine burst is the whole pool of 105, so one
opening kills it) and `melee-bleeds-no-more-on-the-bulwark` (blade takes 45,
carbine 12: 3.8 times). The builders had held the blood band with
`initial_cooldown` 7.0, a bulwark that never cuts; withdrawn.

**What a gun must do and a blade does.** A gun holds fire at the slab, keeps
away and shoots the opening; fire into the guard costs it 20 percent in seconds
and a third more blood. The Light lands 0.13 s into the cut, so no reaction
dodges the first cut. A blade just sprints in and wins quicker, bleeding more.

**Decided for the user and easy to change.** Every figure of the row (bolts.share
0.97 where the brief said a tenth: at 0.90 holding fire killed it as quickly as
waiting); the one bulwark at 24 m in the default fight; the slab (flat orange
cuboid, leaned out when down); the passive start (initial_cooldown) left active.

**Left rough.** The "Blocked" word never showed in the stills (cause not found); the slab reads as a door; `behind-carbine-vs-bulwark` is left out; the duelist's replay rig has no blade; no test that `Seen` cannot move the bulwark.

### Step 9, second part again: a blade holds the bulwark (2026-10-14)

The row above left melee the quicker answer and the bloodier one, because the
bulwark cut the moment it arrived and a cut on its slab cost it nothing it
missed. Three rules of its brain, one figure new, and the fight is the one the
step was for. No still was retaken: nothing drawn changed.

- **Its first attack counts from its arrival.** `initial_cooldown` (0.4 to
  0.9) is now the seconds it stands in reach behind the guard before its first
  cut (`OpponentBrain::arrived`), not seconds from the start of the fight.
- **A cut on the guard holds it there.** `Combatant::guard_met_for` is the
  seconds since the guard last met a cut (a bolt is not counted), and the
  bulwark does not lower its guard to attack inside `pinned` (new, 0.9) of one.
  A blade that keeps cutting keeps it behind the slab; a gun cannot.
- **A broken guard stays down.** After a stagger the guard is down for
  `stagger_cooldown` (0.8 to 1.2) as well as the attack, so a break opens it
  for 1.6 s, and `guard.stamina` (1.40 to 2.00) makes the third light the one
  that breaks it. A bolt still spends 0.15 a point.

opponents.ron is version 7. The health figure tried beside these (125, so one
opening is not a carbine's whole fight) slowed every side alike and was left
out.

| Preset (15 m, 60 bouts, seeds 1 / 101 / 501) | win | s | taken |
|---|---|---|---|
| punish-carbine | 1.00 | 6.27 | 11.7 / 11.0 / 12.4 |
| standing-carbine | 1.00 | 8.78 / 8.79 / 8.95 | 15.2 / 16.6 / 15.6 |
| punish pistol / rifle (seed 1) | 1.00 | 10.69 / 8.27 | 24.3 / 11.7 |
| blade, lights only | 1.00 | 4.33 / 4.36 / 4.32 | 5.1 / 6.5 / 6.3 |
| greatblade, lights only (seed 1) | 1.00 | 4.59 | 14.0 |
| glaive, lights only (seed 1) | 1.00 | 4.11 | 0.0 |
| greatblade, heavy cut (seed 1) | 1.00 | 3.25 | 0.0 |
| kit blade | 1.00 | 4.34 / 3.80 / 4.55 | 22.1 / 25.9 / 19.4 |
| kit greatblade / glaive (seed 1) | 1.00 | 7.67 / 5.42 | 14.0 / 14.0 |
| blade behind its own guard | 0.85 / 0.90 / 0.92 | 33 / 31 / 25 | 81 / 79 / 67 |

**Bands.** New and chosen: `a-pressing-blade-is-quicker-on-the-bulwark` (the
lights-only blade over the punishing carbine in seconds, at most 0.85: 0.69,
0.70, 0.69) and `a-pressing-blade-bleeds-less-on-the-bulwark` (in damage taken,
at most 0.8: 0.44, 0.59, 0.51). `melee-is-quicker-on-the-bulwark` reads 0.69,
0.61, 0.73, `the-bulwark-costs-a-gun` 3.7 to 3.9, `holding-fire-into-the-
bulwark-costs-time` 1.40 to 1.43. Still unchosen: `fire-into-the-bulwark-is-
wasted` (1.00) and `melee-bleeds-no-more-on-the-bulwark`, which is the kit
blade's and reads 1.6 to 2.4 where it read 3.8.

**What it says.** Keep cutting and the bulwark is held, broken on the third
light and cut down in the opening: quicker than any gun and for half the blood.
A kit's abilities are quicker still in some bouts and bleed more, because the
heavy cut's 0.85 s lets the hold run out; the kit greatblade is slower than the
carbine (its three abilities are the abilities pass's). Standing behind your
own guard and waiting is the wrong hand: the bulwark never parries and is not
opened by patience.

**Decided for the user and easy to change.** The three rules; `pinned` 0.9 (at
0.6 a greatblade's lights do not hold it); the break on the third light; the
1.6 s a break opens; that the plain lights, not the kit, are what the two new
bands read.

### Step 9, third part: the abilities put right (2026-10-14)

Figures: the worth page, 60 bouts, seed 1; bands at seeds 1, 101, 501. abilities.ron is version 5.
One new field: `StatusRow.damage_taken` (read once in `Combatant::take`, after the guard; fortified 0.5).

- **focus**: instant now, so a standing rifle loses nothing (1.00). In a fight of one opponent it is strong,
  and that is the page's row: the rifle kills in two shots and five seconds of focus is both of them with no
  stop to aim, 2.70 s against 4.95 s on the marksman and 2.3 to 2.5 s against 4.6 s on the sentry, rusher and
  duelist (`ability-focus-duel`, unchosen, 0.55). No dial of the row brings it under the ceiling: down to
  2.0 s it reads the same and at 1.5 s the hand is slower than without it. A standing rifle takes 3.13 s and
  42 damage there, so focus beats standing still on both counts. On two posts at 60 m, a fight longer than its
  cooldown, it reads 1.13 (the chosen band). Against the bulwark it changes nothing (8.27 s, 11.7 taken).
- **kinetic_absorb**: halves every blow and holds posture, 2.0 s (2.5 read 0.60, under the floor, once a kit
  kept stamina for its cuts). Damage taken by the greatblade kit against the duelist 0.68 (42.6 against 62.6),
  and the fight is 5.1 s against 7.4 s. Against the bulwark it does nothing (14.0 against 14.0;
  `ability-kinetic-absorb-bulwark`, unchosen).
- **butt_strike**: it has a job now. Reach 2.2 (the bulwark holds 2.0 m off), windup 0.12, length 0.40,
  posture 105: pressed as a cut begins it lands first and breaks a full posture, so the punishing carbine
  takes 0.0 from the bulwark where it took 11.7, for 0.4 s more (6.70 s against 6.27 s). Under 100 posture it
  breaks nothing and reads worse than no press. Chosen band `ability-butt-strike` (at most 0.5, all or
  nothing by nature). Against a rusher from 2 m it only costs time (1.20 s against 1.00 s): the carbine kills
  before the cut. Steadiness 3's posture x1.2 buys nothing against a posture pool of 100.
- **vault**: a 6.5 m leap with a landing that breaks more posture (not harder). Sentry from 27 m: the control
  takes two bolts (25.8) and the vaulting kit one (12.0) in every bout, 0.47 at all three seeds and at any
  leap from 5.5 to 6.5 m, so the band's floor is drawn at one bolt of two (0.40). Marksman from 60 m it opens
  a fight the control cannot (win 1.00 against 0.3 to 0.5; the control swings 12 cuts for 34 damage at a
  target that backs away), unchosen. The 10 m leap read 2.4 taken against 25 and was dropped.
- **rapid_reload**: 1.08, 1.08, 1.06 on the carbine against two dummies. Slight by nature; more would need a
  second effect, which is not built.
- **explosive_shot**: 0.90 to 0.91 on seconds against the bulwark; two dummies 1.12 (unchosen).
- **armour_break**: why the duelist fight was 3.2 times faster: the blow, not the status (shield_locked off,
  same time), landing as the duelist begins a cut. Review found the greatblade pair certifies a pause (a
  press with no damage and no status reads 0.79; a melee Hit is a directed cut and a guard meets it), so
  windup, length, cancel went back to 0.30, 0.70, 0.55, that band is unchosen (reads 0.66) and the page reads
  the rifle against the marksman: 3.88 s against 4.95 s (6.65 s with the blow zeroed). The rifle takes 42.5
  from the marksman where its control takes 1.4. On the greatblade it loses time on the rusher and bulwark.
- **disarm**: the press came as the cut began and a cut under way is not stopped, so 0.55 s of commitment cost
  a blow. Quicker hit (0.10, 0.35) with 3 s: damage taken on the duelist 0.83, 0.85, 0.85, but the status adds
  four points only (0.88 without it) and makes the fight 45% longer. Against the bulwark a disarmed bulwark
  never cuts (0.00 against 8.17 taken, 5.85 s against 4.34 s): unchosen, out of band on the strong side.
- **reflect_shot**: eight seconds at twice the strength; guarding blade against the sentry at 12 m 1.28, 1.29,
  1.30. The closing kit blade never raises a guard and meets no bolt, so the 27 m and 60 m pairs read 1.00
  (unchosen); the share of bolts sent back was not measured. In the 12 m walk both bolts met went back.
- **sweeping_cut**: both rushers were inside the arc; the old 1.1 was no more than the light chain, pressed once
  a bout. Damage 1.8: two rushers 1.39, 1.36, 1.42; one rusher 1.00 (band 0.97 to 1.03). Elsewhere it is a
  single-target button (glaive on the duelist 3.24 s against 3.72), and the presser has no two-body rule.
- **combat_slide**, **shield_recharge**: not tuned. The slide reads 0.67 here, 0.29 at seed 101 and 0.54 at
  the review's: a ratio of three hits in a hundred bouts. Shield recharge reads 1.65 at 240 bouts (1.62 to
  1.68): over 1.60 and no dial of the row enters the refill. Both for the user.
- **Blade master**: stays unchosen. As a master plays it (GuardAndCut, parry, heavy_cut, thrust, disarm) it wins
  0.02, 0.05, 0.03 of 60; it meets 0.30 blocks and 0.88 parries of the novice's 6.2 cuts, so its guard boxes
  are barely read. The boxes of no worth are not named (that wants a `balance sweep`).
- **Presser**: a status goes when it can help (focused for a loose shot, fortified before a cut inside 1.5
  reach or its own heavy, reflecting at a guard facing bolts); a tight cone fires on the move; a melee hand
  waits for a raised guard to drop before a Hit or Strike. Two artifacts found after the review and fixed. A
  cutting hand now keeps the stamina of its next cut: the greatblade kit pressed all three abilities in a row
  and stood waiting on its breath (5.2 s taking 41 on the rusher against 3.4 s taking 27.5 with an empty
  bar). And an instant press no longer takes the tick's trigger pull away, which had cost the focused rifle
  its one opening on the bulwark (11.2 s and two cuts taken; 8.27 s and one now, as without it). The
  greatblade kit now reads 4.1 s taking 17 on the rusher (slower than its lights, for ten points less
  blood), 5.8 s taking 43 on the duelist (lights 7.4 s taking 62) and 4.1 s taking 14 on the bulwark (lights
  4.6 s).
- **The nineteen** (page): 6 strong (butt_strike, focus, kinetic_absorb at +47%, shield_recharge, vault,
  combat_slide), 11 pay, rapid_reload slight, thrust opens a fight, none dead, none unread. The page marks
  strong from +40% and the bands allow 1.6, so kinetic_absorb is strong on the page and inside its band. Every
  says line and the six status lines were rewritten, without figures the card shows.
- **Log**: assets/balance/combat_tuning.ron, one entry a dial group.

**Decided for the user and easy to change.** Focus left at five seconds though it is strong in a duel;
armour break's page row is the rifle's; kinetic_absorb at 2.0 s; vault 6.5 m rather than 10 and its floor at
one bolt of two; the butt strike as a certain stagger on an unguarded foe (posture 105).

**Left rough.** The presser flags (loose shot, cut coming, slow cut due) are built inline in the sight and
tested through hand-built sights; cut coming, slow cut due and the stamina reserve are pure functions with tests, the instant press has none.
combat_slide (a ratio of a few hits) and shield_recharge (1.65) are out of band. Sweeping cut is a single
target button outside the rusher fight and the presser has no two-body rule. The closing kit blade never
guards, so reflect shot is read only on the 12 m guard walk. The Blade master is unchosen (0.45 as a cutter,
0.02 behind its guard) and the boxes of no worth are not named.

## Decided by the user (2026-10-12)

- Pistols "primarily single shot medium range", carbines "burst centric",
  rifles "long range sniping".
- "melee weapons need to be first order choices, in line with swg/dark
  souls". All three melee classes in the first pass. Guard on the right
  mouse button. Melee weapons take parts.
- A bar per weapon. Abilities cost stamina. The rifle belongs to the Ranger
  elite. Full auto on the carbine is "ok, but needs a penalty". Codex's icon
  files are ready to use.

## Decided by default, easy to change

- Every number above, and which box grants what.
- Three slots a weapon and two utility; the keys.
- The full auto price is the cone alone.
- The class names Blade, Greatblade and Glaive, and the tree keeping the
  name Blade.
- Two mounts a melee weapon, named grip and edge.
- The loadout changes anywhere out of a fight, not only at a bench place.
- The survey trainer teaches Ranger until the Institute has a place.
- Only Long shots of the Ranger is made real.
- Wilderness XP from rifle damage and long hits.
- The heavy cut is an ability on the bar, not a held button.
- Step 7 kept the Blade's box ids and renamed only what is read (One hand,
  Guard, Greatblade, Glaive), so no save is migrated and nothing refunded.
- A gun bearer is "caught" by a blade (`melee.ron`'s `caught` row), closers
  cover their user, reflect shot is instant (step 5c).
- The greatblade and the glaive share the blade's mark on the bar.
- The marksman and the duelist, every figure of their rows, the one marksman
  at 60 m in the default fight, the beam and the lock's flare.
- Melee is measured against the game's own opponents, not against a
  player's carbine in a policy's hands.

## Open, for the user

- **Melee beside the guns.** `docs/balance/combat-worth.md` says it in one
  table. Against the sentry, rusher, marksman and duelist every gun is still
  quicker and safer. Against the bulwark (step 9) melee is the answer: the
  glaive's kit kills in 3.6 s taking nothing, the greatblade's in 4.1 s taking
  14, the blade's plain lights in 4.3 s taking 6 (its kit 22), where the
  best-played carbine takes 6.3 s and 11.7, or nothing with a butt strike.
- **The duelist's rest.** It is 0.5 (opponents.ron version 5), by the rule of
  step 9: the guarding blade wins 0.88 of its bouts inside 45 s at 0.5 and 1.00
  at 0.3, so there was no reason to leave 0.5. The three bands that moved
  whole on this dial as one trajectory are distributions now (the table above).
- **A blade in reach needs a quick hand.** Against a carbine from 2 m a blade
  wins 0.92 with a hand that draws nothing, 0.82 with the shipped lag of 0 to
  0.08 s and 0.50 with a lag of 0 to 0.25 s before each opening. The game buffers
  a press, so a practised hand is the shipped one; a slow player is a coin.
- **The pistol's middle.** Standing at a standing dummy the rifle edges it at
  30 m (48.0 against 44.4 damage a second). In a moving fight at 30 m the
  pistol beats the rifle hand 0.87 and is the quickest gun on the sentry.
  A tighter pistol cone would settle the dummy too, and would break the older
  band that holds the pistol to 1.6 times the carbine at that range.
- **Abilities to look at.** Six read strong on the page. Focus halves a rifle duel (two free shots) and is
  1.13 in a long fight. The butt strike denies the bulwark its one cut. Vault spares one bolt of two and wins
  the marksman fight outright. Shield recharge (1.65) and combat slide (a ratio of a few hits) are over the
  ceiling and were not tuned. The closing kit blade's reflect shot and the Blade master are unresolved
  (step 9, third part).
- The scope is checked in stills only. The lock's flare is a flat square.

- New melee clips. The three classes share the blade's until someone makes
  them, and nothing in this ticket does.
- Whether an opponent should use abilities at all, past the marksman and the
  guard of step 8.
- Medic, Rigger and the other elites get abilities when their trees are
  made real; the utility slots are where theirs will go.
