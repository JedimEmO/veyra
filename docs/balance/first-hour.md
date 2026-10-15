# The first hour

What a new household does in its first hour, in minutes, from the shipped data
and world (seed 42). `just balance field` prints it (`--write <file>` saves
Markdown, `--questions` adds the two tables below); `tools/balance/tests/
first_hour.rs` holds it to the bands in `assets/balance/first_hour.ron`.

## What the first hour is

The household has 2500 credits, a first day's store and two people who matter:
Ilse Marrow, who surveys, and Tovan Reyes, who builds. Ilse buys the bike,
finds a conductor and a structural metal and samples a harness's and a frame's
worth. Tovan makes the harness, the first carbine and five cell cores that
Pelagic buys. Ilse then rides for a metal that opens the muzzle mount, plants a
harvester, and Tovan makes a second carbine; she rides for one that opens the
fore end, collects the harvester, and Tovan makes a third. The path is data you
can reorder. A step the rules refuse is a row that says why and stops the clock.

Every figure is a sim function (the bike's speed, a ping, a session's lines and
points, a sale, a machine, a box's XP) plus arithmetic on the file's
assumptions. A few rules the model works itself: see "Where the model is thin".

## The first hour after tuning

Minutes are the clock after the step; a switch of member (20 s) is left out.

| Min | What | Purse | Result |
| --- | --- | --- | --- |
| 0.7 | Buy courier_bike *bike* | 2050 | Courier bike for 450 |
| 1.9 | Ride to nearest conductor | 2050 | Scobie red conductor, 988 m from the city |
| 2.8 | Find *first_find* | 2050 | 4 pings and 267 m of walking to the lead |
| 3.4 | Sample 45 | 2050 | 45 units at 550 per-mille |
| 3.7 | Ride to nearest structural_metal | 2050 | Fallow light alloy, 964 m from the city |
| 4.6 | Find | 2050 | 4 pings and 267 m of walking to the lead |
| 5.7 | Sample 80 | 2050 | 80 units at 550 per-mille |
| 7.4 | Home | 2050 | home |
| 8.5 | Craft harness | 2050 | Harness conductance 694 |
| 9.2 | Craft patrol_carbine *first_made_carbine* | 2050 | interval 596, durability 331, moddability 480; 4/6 mounts open, interval x1.06 |
| 13.0 | Craft cell_core x5 | 2050 | Cell core charge_density 573, stability 216 |
| 13.3 | Sell 5 cell_core *first_sale* | 2690 | 5 sold to Pelagic Works for 640 |
| 13.6 | Learn fabricator.batches.1 *fabricator_first_box* | 2690 | Tovan Reyes learns fabricator.batches.1 |
| 14.3 | Learn surveyor.deposits.2 *surveyor_second_box* | 2690 | Ilse Marrow learns surveyor.deposits.2 |
| 14.6 | Buy field_harvester | 2390 | Field harvester for 300 |
| 16.9 | Ride to nearest structural_metal with moddability 600+ | 2390 | Selby ferrous ore, 1851 m from the city |
| 17.8 | Find | 2390 | 4 pings and 267 m of walking to the lead |
| 18.8 | Sample 80 | 2390 | 84 units at 545 per-mille |
| 19.1 | Plant | 2390 | 654 units an hour, 1000 in the hopper |
| 22.0 | Home | 2390 | home |
| 23.1 | Craft harness | 2390 | Harness conductance 723 |
| 23.8 | Craft patrol_carbine *muzzle_carbine* | 2390 | interval 547, durability 419, moddability 691; 5/6 mounts open, interval x1.08 |
| 30.0 | Ride to nearest structural_metal with moddability 800+ | 2390 | Blane ferrous ore, 4527 m from the city |
| 31.0 | Find | 2390 | 4 pings and 267 m of walking to the lead |
| 31.9 | Sample 80 | 2390 | 84 units at 550 per-mille |
| 36.0 | Collect *first_harvest* | 2390 | 184 units after 17 min |
| 38.9 | Home | 2390 | home |
| 40.0 | Craft harness | 2390 | Harness conductance 723 |
| 40.8 | Craft patrol_carbine *fore_end_carbine* | 2390 | interval 569, durability 420, moddability 833; 6/6 mounts open, interval x1.07 |


Bands, minute reached (by): bike 0.7 (3); first find 2.8 (8); first made
carbine 9.2 (25); muzzle 23.8 (45); fore end 40.8 (75); Surveyor second box 14.3
(20); Fabricator first box 13.6 (30); first sale 13.3 (30); first harvest 36.1
(60). All held, none waived. Lowest purse 2050.

## Before

Before anything was moved every band above held at the same minutes and one
figure failed: the harvester. An hour of a store grade machine at 550 per-mille
made 198 units, 3.3 minutes of an untrained hand, against 10 to 25. Nothing
else was out, so nothing else was tuned.

## What was moved

| File | Dial | Old | New | Band it closed | What else it moved |
| --- | --- | --- | --- | --- | --- |
| `assets/resources/harvest.ron` | `rate` | 360 | 1200 | harvester worth: 3.3 to 11.0 min | A machine makes 1200 an hour at 1000 per-mille, 660 at 550. The Stations boxes scale it as before: the full branch is three machines at 1875 an hour each. A machine on strong metal beside Pelagic's intake now earns its 300 back in minutes (estimated, not run by the model). |
| `assets/resources/harvest.ron` | `hopper` | 300 | 1000 | harvester hopper: 1.52 h against 1.5 | A full hopper is more than a pack takes at once, so a collect takes what fits and leaves the rest. The full Stations branch holds 2250. |
| `assets/resources/harvest.ron` | `unattended` | 1.0 | removed | none | Nothing read it. Its check and test went. |

Ticket 0009's step 6 table (360, 203 and 54 units an hour, "three machines at
563") is stale; step 7's section supersedes it. The sim's harvest tests, the
registry's dial check, the game's `--harvester` test and three client unit
tests were updated to the new numbers.

## What a Surveyor box buys

Minutes at the end of the path with only that box added or taken off, its
own prerequisites kept on both sides (Learn steps left out of both).

| Box | Without | With | Saved |
| --- | --- | --- | --- |
| surveyor.deposits.1 | 42.1 | 40.4 | 1.7 |
| surveyor.deposits.2 | 40.4 | 39.9 | 0.5 |
| surveyor.deposits.3 | 39.9 | 38.3 | 1.6 |
| surveyor.deposits.4 | 38.3 | 36.9 | 1.4 |
| surveyor.reading.4 | 40.4 | 36.1 | 4.2 |
| surveyor.routes.2 | 40.3 | 38.5 | 1.8 |
| surveyor.routes.4 | 38.5 | 38.5 | 0.0 |

Reading 4 matters most, 4.2 minutes of 41: it reads a deposit's peak, so a find
is one ping and no walk. Deposits 1, 3 and 4 and Routes 2 save one and a half to
two minutes each; Deposits 2 half a minute. Routes 4 saves nothing: it speeds
roads and the path rides open ground. A box is a minute or two in an hour, not
a different game. Reading 4 assumes nobody walks from a deposit's edge to its
centre.

## A made carbine against a bought one

| Mark | Minute | Made: mounts open, interval factor | Bought: mounts open, interval factor | Bought still worse |
| --- | --- | --- | --- | --- |
| first_made_carbine | 9.2 | 4/6, x1.06, 331 | 4/6, x1.10, 500 | no |
| muzzle_carbine | 23.8 | 5/6, x1.08, 419 | 4/6, x1.10, 500 | no |
| fore_end_carbine | 40.8 | 6/6, x1.07, 420 | 4/6, x1.10, 500 | no |

Each cell is mounts open, interval factor (lower fires faster), durability. A
bought carbine opens four of six, fires at 1.10 and has durability 500. The made
one opens more from the muzzle on and fires faster, but is worse on durability
at every mark (331, 419, 420): the model spends the crafter's points on
moddability first, then the interval (the file's `spend`). So the bought one is
not worse on every line at any mark. The first made carbine opens no mount a
bought one lacks. The muzzle needs a metal 1.9 km out, the fore end 4.5 km.

## Where it still drags

- It is short: 41 minutes against a 75 minute band, so the bands hold nothing
  but the harvester. Tightening them is the user's call.
- Riding is 21 of the 41 minutes. The worst stretch is minute 24 to 39: 12 of
  14 minutes in the saddle with nothing to decide. The off-road ride speed (0.85
  of 18 m/s, detour 1.25) is an assumption: the only scripted ride is on a road.
- Five cell cores in a row is 3.75 minutes of the same click.
- The first made carbine is no reward: the first day's store holds what one
  needs, it opens the same four mounts and lasts less. The muzzle at 24 minutes
  is the first real one.
- No box changes what the player does inside the hour. Tovan starts with 900
  Fabrication XP and Batches 1 needs 150, so `fabricator_first_box` measures
  nothing. Ilse reaches Deposits 3 near minute 30 and the path never learns it.
- The harvester is not worth its trip: 184 units after 17 minutes is about two
  minutes of Ilse's own sampling, and nothing on the path uses or sells it.

## Where the model is thin

Mounts are worked from `opens_at`, not the registry's `mount_opens`. A sale's
and a planting's checks are subsets of the sim's (`check_sale`, `can_plant`).
The first day's lots are drawn from the city's centre, not the game's spawn.
`harvest::made_in` copies the tick's arithmetic and no sim test ties the two.
Away from home a buy, sale, session, lesson or switch is refused; each return
with lots costs a locker visit. Rework is held by
`apps/game/src/household/tests/rework.rs`, not by the path.

## Assumptions

`assets/balance/first_hour.ron` holds every number no rule says, each with a
comment on where it was measured: the detour (1.25), the bike's mean over its
top speed (0.85), the pings (5) and metres (400) of a find, the concentration a
walked lead is sampled at (550), a session (45 s) and a screen (20 s), the order
points are spent in, the path and the bands. A longer ping scales pings and
walking by the base range over the new one (not measured). Points are spent at
the mean lift of a steady hand on the base row. Rerun `just balance field` and
`cargo test -p balance --test first_hour` after a change to survey, harvest,
session, price, XP or deposit data.
