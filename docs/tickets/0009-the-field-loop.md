# 0009: The field loop: surveying by hand, a hoverbike, professions that mean something

Status: planned 2026-10-12, not started. Written after the user played the
slice ticket 0007 built ("things seem to work") and asked for a better
experience from it: "the walk distances are horrendous", "I want the player
to be surveying themselves (from a skill for sampling and surveying, as per
swg)", and "more profession impl. (xp, meaningful boxes of modifiers and
schematics, trainers etc. being present)". The plan below was put to them
in outline and accepted ("this plan sounds good"), with one change: no
shuttle travel yet ("I have some other ideas there"), a hoverbike instead.

Singleplayer still holds: no protocol, `world_version` or
`GENERATOR_VERSION` change, and nothing under geology, sites or flora is
edited. Deposits are in no version, so the ground's resources may move.

## What is wrong with the slice as played

- **The walk.** Walking is 5.5 m/s and a sprint 8.5. Deposits stand on an
  800 m lattice, and metal good enough for a carbine with a third mount is
  3 to 8 km from Port Serein: 6 to 16 minutes each way, on foot, with
  nothing to do on the way.
- **Surveying is reading a list.** The survey office publishes every
  deposit within 16 km with its exact figures, and a post stands on each.
  Nothing is found and no skill is used.
- **Most trees are words.** Marksman, Blade and Weaponsmith are real. The
  Surveyor and the Fabricator are placeholders whose boxes change nothing,
  seven schematics exist, and the only trainers are the two combat ones.

## The loop this ticket builds

Ride out on a bike. Ping the ground with a tool to find where a resource
is richest. Sample it by hand, or plant a harvester and come back. Ride
home, make something better than the store sells, and learn a box that
makes the next trip or the next batch better.

## 1. The ground: deposits become areas

- A deposit keeps its identity, its type, its quality figures and its lot
  origin, exactly as now. It gains a **radius** and a **concentration**
  that is highest at its centre and falls to nothing at its edge. Still a
  pure function of the seed, the dials and the samples.
- The lattice shrinks from 800 m to about 400 m, so ordinary material is
  close and only the best is a trip. A changed `cell_m` renames every
  deposit; the save's survey record is already stamped with the lattice
  and is dropped cleanly in a world it was not made in.
- The fee system goes: no concession, commons or claim, no terms on a
  deposit and nothing paid to take from the ground. Survey posts and the
  published list go too, once the tool of part 2 replaces them (step 3;
  until then a post stands and is free). Everything is found with the tool.
- The first day's stock has a basic survey tool, and the arrival knows
  Surveyor novice, so the first minute is a ping.
- Every class has at least one deposit within 500 m of the city's bench
  place, held by a test as the carbine metal is today, so the first finds
  are a short walk and need no bike.

## 2. Surveying by hand

- **The tool.** An item of a new kind, held in a hand like a weapon, with
  one quality line (its range). The first is bought; a better one is made.
- **Ping.** Pick a class, ping. After a short channel the HUD shows the
  concentration of each type of that class on a small grid round the
  player, and marks the strongest reading. Walking the marker up to the
  peak is the search. A `SurveyRequest::Ping`, consumed in `SimSet::Input`
  and answered like every other request.
- **Sample.** Where there is a reading, sample: a few seconds a cycle, a
  few units a cycle, more where the concentration is higher. It replaces
  Draw and the post's screen. Stopped by moving or by being hit.
- **Reading quality is a skill.** With no Reading box a ping names the
  type and the concentration only. Boxes add the quality lines as bands
  (poor, fair, good, fine), then as exact figures, then the comparison
  with the best the household has found.
- **The register is the player's own.** It lists only what the household
  has found, each with what has been learned of it. A deposit pinged but
  not read shows its type and nothing else.
- **Who.** The tool needs Surveyor novice, as a schematic needs its box.
  Surveyor is a basic tree, so any member of the household can learn it.

## 3. The hoverbike

- **What it is.** One vehicle, owned by the household, called and stowed
  rather than parked: a key calls it after a short channel and the player
  is on it; the same key gets off and it is gone. No bike left standing in
  the world in this ticket, so nothing to save but that one is owned.
- **Where it lives.** The sim owns the player, so the ride is a state of
  the player in the sim (`Mount`), asked for by a request in an inbox, and
  a second motion model in `strata_physics::character` beside the walk.
  The client only draws it and moves the camera back.
- **How it moves.** It hovers about a metre up and follows the ground.
  It has momentum: it gathers speed, turns in an arc at speed and slides a
  little. About 30 m/s on a road, 18 m/s on open ground, slower uphill,
  and ground steeper than a set slope stops it. The road bonus reads the
  sample's `cleared`, so the road network the world already has is the
  fast way to go.
- **Water.** It crosses what a walker can wade and stalls over deep water,
  so fords and bridges still matter.
- **Trees and walls.** A hit at speed stops it, throws the rider off and
  costs the bike condition, on the wear rules that exist. A kit mends it.
- **Fighting.** No shooting from the saddle. Being hit dismounts. It
  cannot be called with a hostile aware of the player.
- **Cargo.** It has saddle bags, cells of its own beside the pack's, so a
  trip brings back more than a pack holds.
- **Getting one.** The first is bought from Pelagic's board at a price a
  few sold batches reach. A made one, with lines for top speed, handling
  and hull, comes with the Fabricator's schematics in part 5.
- **Art.** A baked kit like the weapons (`just content`, an `AshlarProp`).
  The rider needs a seated pose, which the character art may not have;
  the step finds out first and says what it used.
- **Streaming.** The benches fly at 200 m/s, so 30 m/s is inside what the
  streamer already keeps up with. The step still measures a ride.

## 4. The Surveyor made real

XP (Survey): a ping over ground not pinged before, each sampling cycle, a
first find of a type, a find that beats the household's best on a line.

| Branch | What its four boxes do |
| --- | --- |
| Deposits | Ping range (64, 128, 256, 512 m), units a sampling cycle |
| Reading | Bands, exact figures, the best-found comparison, finer grid |
| Stations | Harvesters: how many may stand, how much each holds, its rate |
| Routes | The bike: off-road speed, the slope it takes, condition lost in a hit |

A Surveyor trainer stands at a survey office in Port Serein, not in the
row at the bench place.

## 5. The Fabricator made real, and things to make

XP (Fabrication) already flows from a finished bench session.

| Branch | What its four boxes do |
| --- | --- |
| Fixtures | Schematics for parts and modules; a point more to spend at the higher boxes |
| Batches | The fabricator: runs a blueprint, played seconds a unit, the unattended share |
| Materials | The assembly share, and how far a poor lot is forgiven |
| Repair | The field repair's amount, kits that restore more, the bike's upkeep |

The schematic list grows from seven to about twenty, each granted by a
named box. The step fixes the exact boxes; the list is:

| Made thing | Tree |
| --- | --- |
| Cell core, harness, field repair kit (exist) | Fabricator |
| Survey tool, sample canister, pack bag | Fabricator |
| Lens, then the reflex sight | Fabricator, Weaponsmith |
| Compensator, grip, stock, fore end | Weaponsmith |
| Extended feeds, mount blank, patrol carbine (exist) | Weaponsmith |
| Service pistol | Weaponsmith |
| Drive coil, hull plate, then the hoverbike | Fabricator |
| Harvester | Fabricator |

A Fabricator trainer stands at the works in Port Serein. The bench still
teaches the trees it teaches today.

## 6. Harvesters

- Planted on a deposit the household has found, by a member with a
  Stations box. It fills over played time at a rate set by the
  concentration where it stands, into its own hopper, with the deposit's
  quality figures exactly.
- The same machinery as the fabricator's run: nominal ticks of played
  time, a pause when full, saved in the household file with a serde
  default.
- Collected by riding out to it. It shows in the register with what it
  holds.

## Steps

Each is built, checked, captured and committed on its own, with targeted
checks, as in 0007. The full `just ci` runs once at the end.

| Step | Builds | Done when |
| --- | --- | --- |
| 1 | Area deposits, concentration, the 400 m lattice, the fee system and its terms removed | The worldmap's deposits layer draws areas; a test holds every class to a deposit within 500 m of the bench; a draw costs nothing; the script still makes its carbine |
| 2 | The hoverbike: mount state, motion model, camera, baked kit, bought at the board | A scripted ride along Port Serein's road covers its length at road speed with no streaming stall; a tree hit dismounts and costs condition; a still of the rider |
| 3 | The tool, ping, sample, the personal register | A scripted member finds an unpublished deposit by pings alone and samples a harness's worth; no post stands anywhere and the register lists only what was found |
| 4 | Surveyor boxes, Survey XP, the trainer at the survey office | Each box changes a number a test reads; the script earns and learns one |
| 5 | Fabricator boxes, the schematic list, the trainer at the works | Every listed thing is made by the script or a test; no box is words only |
| 6 | Harvesters | One planted, filled over played time, read back from a save, collected |
| 7 | A balance pass over the whole loop | A written first hour: what a new household does, in minutes, to its first made carbine with a third mount |

Steps 1 and 2 are independent of the rest and answer the walk first.

## As built

### Step 1: deposits as areas, the fees gone (2026-10-12)

Stills: docs/design/field/step1-assay.png (a post's screen, no terms or cost),
step1-register.png (rows without fees, "within 8 km"), step1-map.png (the
deposits layer as discs with an edge ring, 5 km round Port Serein).

What changed:
- `deposits.ron` is version 2: lattice 400 m, no terms, `area`, post stand and
  drop 800/1000 m, register range 8 km. The registry checks the area.
- A `Deposit` has `radius_m` and `peak` (new salts 4 and 5, old salts kept),
  `concentration` and `DepositField::covering`; the terms and networks code is
  gone, so a query bakes no land mass. `concentration_here` is in the sim.
- A draw takes nothing; `cost`, the short-purse refusal, the TERMS box and the
  fee words are gone. `LotOrigin` has no terms and reads past an old one.
- A save made on the old 800 m lattice drops its survey record (visited,
  tracked) and restamps, so the run records again; another seed is kept.

Nearest deposit to the city anchor per class (seed 42): Reactive 681 m, Brine
736 m, Structural metal 769 m, Conductor 988 m. The starter test holds 1000 m,
not 500: the city's developed ground refuses candidates. From the terminal the
register shows the nearest conductor at 1.1 km.

Pinned figures that moved: the first alloy is Mardle light alloy (862 m), its
ceiling 420 (was under 400), so the scripted carbine opens 4 of 6 mounts (was
3) and the script's rework step no longer runs (reworks 1 to none). Purses
after draws 2470/2440/2410/1900 to 2500. Register captions 16 KM to 8 KM.
Draw label "Draw 50 units · 150 credits" to "Draw 50 units". The
within-twelve-kilometres name test is now six (the name block is 16 cells).

Decided for the user and easy to change:
- Lattice 400 m, area radius 120 to 260 m, peak 350 to 1000: `deposits.ron`.
- Post stand 800 m, drop 1000 m, register range 8 km: `deposits.ron`.
- Worldmap disc opacity 0.15 to 0.65 by peak: literals in `apps/worldmap`.
- The assay keeps the credits readout, since buying uses the purse.

Left rough:
- The first-day carbine matches a bought one's four mounts; rework has no
  end-to-end script (step 7's balance pass; `--crafted-carbine` under 400 with
  `--blanks` can cover it).
- Areas spill over the sea and the town's edge; `covering` gives a
  concentration there. Matters for the ping and sample of step 3.
- Some labels collide on the map.

### Step 2, first half: the ride (2026-10-12)

Stills: docs/design/field/step2-road.png (mounted at the road's far end, camera
behind at 6 m, "0 km/h" readout, no reticle), step2-side.png (the bike's
shape and the seat, boom 4 m), step2-foot.png (on foot, nothing changed).
No still is at speed; lean and pitch are unit-tested only.

What was built:
- `assets/vehicles/courier_bike.vehicle.ron` and `strata_registry::vehicle`;
  `strata_sim::ride` (`Stable`, `Riding`, `RideRequest`, `can_call`,
  `RideNotice`); the motor is `CharacterController::ride` beside `step`.
- The bike's velocity lives in `Riding`, so a dismount or a throw drops it.
  A crash is judged on the slide's clipped velocity, so it does not depend on
  where in the tick the wall is met; a kerb stepped over is no wall. The
  bike's wall is its own `stop_deg`, not the walker's 50 degrees. In the air
  it cannot gather speed.
- `veyra_buildings::vehicle` bakes to `generated/vehicles/veyra/`; V calls,
  gets off or cancels. A terminal switch takes the rider off; no reticle.

Scripted ride (headless, `--at road`, inward from the far end, 40 s): 1076.9 m,
mean 26.9 m/s (0.90 of road speed), top 30.04, 0 s stalled, 0 crashes, 0
thrown. It never reached the road's end. The test runs on the 60 Hz clock
(about 58 s) so the streamer keeps up.

The rider: the body has no seated clip (run, idle, aim, fire, cuts, dodge,
dead). It holds the idle pose lowered onto the saddle with the thigh and shin
bones folded forward after the animation (`pose_seated`); leg IK, strafe
twist and the run cycle are off. The arms are the idle pose, so from behind
they read as raised.

Decided for the user and easy to change (all `courier_bike.vehicle.ron`):
- speed road 30, ground 18, reverse 4; accel 9, brake 16, drag 3.
- turn slow 150, fast 55 deg/s, grip 6; slope full 14, stop 36 deg.
- water ride 0.5 m, stall 1.1 m, crawl 3; road cleared/developed 0.5.
- crash above 12 m/s losing 0.6; call 1.5 s, clear 40 m; view hover 0.9,
  camera 6.0 (was 9.0, too small), eye 1.6. `--bike` works on the real save.

Left rough:
- Lean, bob, ease, throttle arc and fold angles are consts, not data.
- `can_call`'s InWater reads the water over the ground, so a call from a
  bridge deck over a deep river is refused.
- A call cancelled by moving is silent, and a throw has no toast; both want a
  word.
- No arms on a bar; no still at speed; no test of the switch's dismount;
  `--script-report` accepted with no script; `--mounted --look` points north.

### Step 3: the tool, the ping, the sample (2026-10-12)

Stills (`docs/design/field/`): `step3-survey.png` (the grid after a conductor
ping, Sample here live), `step3-empty.png` (a ping at the city that met
nothing), `step3-sampling.png` (the HUD bar mid sample), `step3-register.png`
(the first lots' deposits as found and sampled).

Built: `survey.ron`, the carried tool (`veyra:tool.survey`, `ItemKind::Tool`),
`Surveyed` as knowledge (`found`, `tracked`, `lead`), `Ping` and `Sample`
requests with their channels, `ui::survey` on G, the lead marker, the
register of found deposits, the card by `Reading`. Posts, Draw and the assay
screen are gone. A reviewer's finds fixed: the YOU mark follows the player
(`PingResult::cell_of`) and the grid says how far back it was pinged; the
untrained reading is Bands as the brief had it; `--ping` and `--script-survey`
are refused on the played household; the script starts 60 m east of the axis
so it must turn to the lead; the `lots.draw` dial and `offers` are deleted.

Scripted survey (`--script-survey conductor`, headless): 5 pings, 401 m
walked, 101 s of play; found Jarrow lode (peak 578), sampled at 565 (0.98 of
the peak), 15 units. `--script-crafting` samples 12 units at its stop.

Decided for the user and easy to change:
- `survey.ron`: ping 2.5 s, 128 m reach, 5x5 grid, floor 20 per-mille; sample
  4 s, 8 units a cycle, stops at 1.5 m; untrained reading Bands; band bounds
  250, 500, 750; `swim_m` 0.85 and `lead_clear_m` 10 are added dials.
- The tool is carried, not held, and works from the pack or a bag; its range
  ends (0.75 and 1.5) are in `survey_tool.item.ron`. Every member's pack
  carries one (the last cell), not one a household.
- Sampling teaches a deposit's figures: a sampled deposit is read Exact.
- `--at deposit` starts at 0.8 of the radius, not the edge, so a ping reads.

Left rough:
- No Stop control on the screen: walking 1.5 m off is the stop.
- The client works a few figures the sim should (the met rows' distance and
  bearing, the lead's distance, `place`'s found filter); the screen rebuilds
  its view every frame, and a ping samples the field 25 times.
- The swimming end and `InWater` read the water depth, so a bridge deck over
  a deep river counts as water; no test ends a sampling by swimming.
- The crafting script's test cannot show its ping: its target was already
  found by the first-day grant. East and west of the grid are tested only in
  the client's walk test, not against a deposit to the east.
- `Phase::ToPost`, `Phase::Post` and `DrawReport` keep their old names, and a
  few doc comments still say post.

### Step 4: Surveyor made real (2026-10-12)

Stills (`docs/design/field/`): `step4-sheet.png` (Ilse Marrow's sheet on the
Surveyor tree at the survey trainer, `--as "Ilse Marrow" --at survey
--open-sheet`; `--open-sheet` now opens on the trainer's tree), `step4-untrained.png`
(the arrival's ping, type only) and `step4-trained.png` (Ilse's: wider
spacing, each line as a band).

Built: the effects `Survey`, `Read`/`Peak` and `Ride` (checked: known field,
grid only a Set to an odd 3..33, finite values, Set and Mul over nought,
`better_pm` over nought), one fold (`learned_effects`, Set then Add then Mul),
`skill()`, `ride::dials`, `rates.survey`, `survey_xp`, the survey trainer
(`--at survey`), Ilse as the surveyor, and on the screen: skill row, lines
as read, Peak row, Stop controls, heading-up grid. Item 17's three tests prove
their names. A reviewer's finds fixed: the met row shows the lines; no XP on an
unkept record; weak registry checks; the trainer moved out from under a banner
(city row spacing 3 m, shift 0.5); the balance box sweep skips survey and ride
boxes.

Boxes (untrained: reach 128 m, 5x5, 8 units, 2.5 s ping, 4 s cycle, Type):
- Novice: units +1, ping time x0.8. Deposits 1 to 4: reach +64, units +2,
  reach +128, units +3 and cycle x0.8. Master: reach +128, ping time x0.6.
- Reading 1 Bands, 2 Exact, 3 grid 7, 4 Peak (centre and bearing).
- Routes 1 to 4: bike call x0.6, off-road speed x1.1, wall +5 degrees, road
  speed x1.1. Stations 1 to 4: nothing yet (harvesters, the next step).

`skill()` (store-grade tool): the arrival 128 m, 5x5, 8 units, Type, 2.5 s;
Ilse (novice, Deposits 1, Reading 1) 192 m, 5x5, 9 units, Bands, 2.0 s; a full
tree 448 m, 7x7, 14 units, Exact, 1.2 s ping, 3.2 s cycle, Peak.

XP: a first find pays 120 and its first cycle 60; a cycle pays 6 times the
concentration (3 at 565 per-mille). Ten minutes of sampling is at most 150
cycles, about 450 plus the 180, so Deposits 2 (400) comes in the first ten
minutes, Deposits 3 (900) in about twenty. The scripted run earned 209 XP as
the arrival and 188 as Ilse.

Decided for the user and easy to change:
- Every number above is data in `trees.ron`; the untrained reading is Type.
- A ping pays `find` and `better` once a ping, not a deposit.
- Sampling rich ground pays as long as the pack has room, with no walking.

Left rough:
- The sheet cuts the novice box at 720p; Stations boxes read "Learn here".
- The grid is north up on a flycam or orbit camera, heading up on the combat one.
- A click in the one frame after a Stop rebuild can send Ping (refused Busy).

### Step 2, second half: owning the bike (2026-10-12)

Stills in `docs/design/field/`: `step2b-board.png` (Pelagic's FOR SALE rows
with 600 credits), `step2b-card.png` (the stores screen, a bike pinned, Take
as ride; from a hand-edited household, no flag puts a bike in the pack),
`step2b-saddle.png` (`--bike --mounted --saddle-stock --open-inventory`: the
RIDE pane, the saddle open) and `step2b-speed.png` (108 km/h from behind on
the road: `--bike --no-hostiles --script-ride --look 1.0 --orbit-distance
3.5 --frames 1050`).

What was built:
- The item `veyra:vehicle.courier_bike` (kind `Vehicle`, price 450, lines
  speed, handling, durability, a condition) and the vehicle file's `lines`,
  `cargo`, `wear` and a fuller `view` and `turn`. `ride::dials` is the one
  place the file, the record's lines, the sheet's boxes and the wear become
  numbers; the mount, the call, `--mounted` and the card all ask it.
- Pelagic's `sells` and `ProcurementRequest::Buy` (`check_purchase`); the
  board's FOR SALE section.
- `Stable { bike: Option<Record>, saddle }`, `RideRequest::{Stable, PutAway}`
  with `can_stable` and `can_put_away`; the saddle is `StockLayout::SADDLE`,
  open mounted or at a locker, asked at every click, shift-click routed.
- A crash wears the bike; `WearRequest::RepairBike` mends it with the kits.
- `can_call` measures the water over the feet; a cancelled call and a throw
  are said; the household file keeps the stable (no version change, an old
  `bike` key reads as a store grade bike).
- The RIDE pane on the pack and stores screens, the bike's card, Take as ride.

Decided for the user and easy to change (`assets/vehicles/courier_bike.vehicle.ron`
unless named): lines speed 0.90 to 1.10 and handling 0.85 to 1.15; cargo 6 x 3;
wear crash 120, speed_cap 3.0, worst_speed 0.6 (a crash costs 125 at the
minimum speed: the wear step of 25 rounds it); the price 450 and the survey
tool's 120 in `pelagic_works.buyer.ron`; the view's lean 20 degrees, pitch
0.45, bob 0.03 m at 2.6 s, ease 0.5 s, follow 8; the turn's throttle arc 100
degrees and dead zone 0.1; `call.feet_m` 0.85.

Left rough:
- The rider is the idle rig with overlays, not a seated clip: the arms reach
  down to the bars, the shins hang beside the body. A seated clip is the fix.
- At 1280x720 with the store open the RIDE pane sits under the bags and the
  saddle's third row is below the body's fold when both Repair and Put away
  are grey (it scrolls into view); the pack-only screen fits.
- A hit during a call is not said: the channel is checked for movement each
  tick and against `can_call` when it ends. `call.feet_m` can drift from the
  capsule's half height. The rider's skill boxes are not in the card's
  figures. The workbench's own Buy refuses a bike: it is sold at a board.
- `RepairRefusal::EmptyHand` is reused for a missing bike.

### Step 5: Fabricator made real (2026-10-12)

Stills in `docs/design/field/`: `step5-sheet.png` (Tovan Reyes at the works
foreman, Fabricator tree), `step5-list.png` (fifteen schematics, grouped, the
untaught greyed with their box, the list scrolling alone with a scrollbar),
`step5-tool.png` (survey tool picked; the slots are not filled, see rough).

| Box | Effect |
| --- | --- |
| novice | assembly share +0.02 |
| Fixtures 1 / 2 / 3 / 4 | run seconds x0.9 / runs x1.25 / unattended +0.15 / seconds x0.8 |
| Batches 1 / 2 / 3 / 4 | +1 point / assembly +0.03 / +1 point / assembly +0.04 |
| Materials 1 / 2 / 3 / 4 | teach coil and survey tool / hull plate / bike / nothing yet (harvesters) |
| Repair 1 / 2 / 3 / 4 | field repair x1.2 / workbench repair uses no kit / x1.3 / wear cost x0.8 |
| master | +1 point, seconds x0.8 |

A full tree reads assembly 690, 6 points, 125 runs, 138 ticks a point, 650
unattended, field kit 1000, no bench kit, a shot at 1600 (dials 600, 3, 100,
240, 500, 700, kit, 2000).

| Schematic | Taught by | Takes |
| --- | --- | --- |
| cell core, harness, field repair kit | nobody | as before |
| mount blank, carbine, extended feed, pistol cell | Weaponsmith frames.2, frames.1, fittings.2, fittings.1 | as before |
| drive coil | Materials 1 | conductor 20, brine 10 |
| hull plate | Materials 2 | structural metal 30 |
| survey tool | Materials 1 | coil, conductor 15, seal set |
| courier bike | Materials 3 | 2 plates, 2 coils, core, metal 60, seal set |
| vigil reflex | fittings.3 | harness, conductor 10, seal set |
| vigil angled grip | fittings.3 | structural metal 20 |
| vigil compensator | fittings.4 | structural metal 25, seal set |
| service pistol | frames.1 | metal 40, harness, seal set |

Review fixes: the schematics column is its own `Scroll` (it could not be
scrolled and cut off five rows at 720p), the panel is 94 % high in every state
so the frame holds still, a test focuses the last row and holds it in view,
and the balance sweep skips `CraftPoints` boxes.

**Decided for the user and easy to change.** Tovan Reyes also knows
Materials 1 (66 of 250 points). Group order Parts, Mods, Tools and vehicles,
Weapons, Kits (the design's put Modules fourth). A counted slot is one pick
with N pieces taken. The compensator weighs `aim_move_speed` on plain Density
(heavier is better); a lighter-is-better `Light(Density)` is one word. The
city row shift is 1.0 spacing for the foreman's end.

**Left rough.** The survey-tool still cannot show filled slots (no stock flag
gives a conductor). The foreman's post was judged from stills behind the
player. The sheet's learn words say "A workbench teaches it" and never name
the foreman. A kit's card reads the plain field amount, not the member's.
The registry does not hold a full sheet's assembly under 0.9 (a sim test
does). The teaching-box test checks the description's words, not a derived
"Teaches" line.

### Step 6: harvesters (2026-10-12)

Stills in `docs/design/field/`: `step6-plant.png` (Ilse Marrow's survey screen
on a found deposit, Plant harvester here live beside Sample here),
`step6-prop.png` (a planted harvester from about 4 m, the F chip showing),
`step6-screen.png` (the machine's screen after half a played hour, 101 of
300), `step6-register.png` (a deposit's row reading "Harvester 101 of 300").

**Built.** `assets/resources/harvest.ron`; the item `veyra:station.field_harvester`
(kind `Harvester`), its schematic (Fabricator Materials 4) and a line in
Pelagic's board; the `Station` effect (`count`, `rate`, `hopper`) on
Surveyor Stations 1 to 4; `strata_sim::harvest` (Plant, Collect, PickUp, Lead,
`can_plant`, `station`, `advance` on the factory's `nominal_ticks`); the prop,
the household save and the three flags in the game; `ui::harvester`, Plant on
the survey screen and the register's holding in the client. Collecting pays 1
Survey XP per ten units. Review fixes: no planting where the survey record is
not kept (the machine would be lost on quit), the register leads to the
machine and not the deposit's centre, Plant is hidden by what is carried and
not by the first refusal, a retuned lattice leaves machines standing, the
`--harvester` flag holds to `min_pm`, a bigger lamp that clears the name.

| Concentration | Units an hour | Full (300) | By hand |
| --- | --- | --- | --- |
| 1000 per-mille | 360 | 50 min | 120 a minute |
| 565 | 203 | 89 min | about 68 a minute |
| 150 (the floor) | 54 | 5 h 33 min | |

A full hopper is 2.5 minutes of hand sampling at a full reading. A harvester
is income while the household is busy elsewhere, not a replacement for
surveying; the whole Stations branch is three machines, 563 an hour each.

**Decided for the user and easy to change** (`harvest.ron` unless noted): rate
360 an hour at 1000 per-mille, hopper 300, `min_pm` 150, `apart_m` 30, `reach_m`
3, `unattended` 1.0 (read by nothing); the item's rate 0.8 to 1.25 and hopper
0.8 to 1.5 (`field_harvester.item.ron`), price 300 (board only, not the
workbench); Stations 1 to 4 add 1, x1.25 rate, 1, 1 and x1.5 hopper
(`trees.ron`); `rates.survey.harvest` 1.

**Left rough.** A machine's prop is stood from a ray cast each frame until its
ground streams (a machine kilometres away retries); keeping the planter's
height with it would end that. The carry of a unit reaches the file only when
a unit is made, so a quit loses under one unit a machine. `unattended` is not
wired. The harvester screen has no Track button (the register's Enter leads
to it). Gear chip of the stores does not list a harvester. The planted body
is the props' near-black material; the lamp and glow carry the state.

### Step 7: the first hour (2026-10-12)

`just balance field` (`tools/balance/src/field.rs`, over
`assets/balance/first_hour.ron`) walks a path of steps over the real rules and
prints a timeline; `--write` saves it as Markdown, `--questions` the two
tables of `docs/balance/first-hour.md`. The `first_hour` test of `tools/balance`
holds it. Minutes against bands (seed 42): bike 0.7 (3), first find
2.8 (8), first made carbine 9.2 (25), muzzle carbine 23.8 (45, five mounts),
fore end carbine 40.8 (75, six), Surveyor second box 14.3 (20), Fabricator first
box 13.6 (30), first sale 13.3 (30), first harvest 36.1 (60). No band waived.

**Dials moved.** `harvest.ron` `rate` 360 to 1200 and `hopper` 300 to 1000
(a store grade machine was 3.3 minutes of an untrained hand an hour against
10 to 25, now 11.0; the hopper holds 1.52 h). `unattended` removed (nothing
read it). This supersedes step 6's table: a machine at 550 per-mille makes 660
an hour, the full Stations branch is three machines at 1875 an hour with a
2250 hopper. Nothing else was out, so nothing else moved.

**Rough edges closed.** The rework runs end to end in a game test
(`household/tests/rework.rs`); `--script-crafting` predicts the saddle's layout
and names its phases ping and sample; the sheet shows a whole tree at 720p; the
survey screen rebuilds only on a change; the locker stores fit 720p with a bike
stabled; `notice_words` reads its words and the full condition from data; the
four missing ride and board tests; bench-taught learn words, "Teaches:" lines
and the kit card's `field_amount_for`; the harvester screen's Track and the
Gear chip. Stills in `docs/design/field/step7-*.png`; the city row stands on
open paving, nothing moved.

**Decided for the user and easy to change.** The bands and the path, both in
`first_hour.ron`; the order a session's points are spent in (`spend`:
moddability, interval, durability, so made carbines are lower on durability
than a bought one's 500); the ride speed off the road (0.85 of 18 m/s, detour
1.25) and the walk a find costs, unmeasured off a road.

**Left rough.** The path is 41 minutes against 75, half of it riding; the first
made carbine opens the bought one's four mounts; Batches 1 is learnable at
minute 0, so its band measures nothing; no box changes what the player does
inside the hour; a harvester is worth its trip only if sold. The model works
mounts, a sale's and a planting's checks itself and copies `harvest::made_in`.
A hit during a bike call is not said (the sim sends no notice). The
`--script-crafting` baseline resync (one) is untraced.

## Decided by the user (2026-10-12)

- Deposits are fixed areas from the seed. No spawns that shift over time.
- A deposit never runs dry.
- Any member can survey once they have learned Surveyor novice.
- Surveyor and Fabricator are made real. The Rigger stays a placeholder,
  so harvesters hang off the Surveyor's Stations branch for now.
- The bike carries cargo.
- The fee system is removed ("that doesn't sound like fun gameplay").
- No shuttle travel yet; they have other ideas there.

## Decided by default, easy to change

- With the fees go the posts and the published list: everything is found
  with the tool, and a new household starts with a basic one and the skill
  to use it. The user asked only for the fees to go; this is what follows
  from it.
- The bike is called and stowed, not parked.
- The bike's cargo is saddle bags: cells of its own, opened beside the
  pack while the rider is on it or stood where it was called, and at a
  locker. Its size is a line of a made bike.

## Open, for the user

- Whether a second rider or a passenger ever matters in singleplayer.
