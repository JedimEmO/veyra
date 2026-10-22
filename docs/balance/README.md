# Balancing

How a combat number is changed here. The principle is section 7 of
[the progression design](../design/progression.md): one model, which is
the sim and the physics the game runs, driven headless by
[`tools/balance`](../../tools/balance/README.md); a change is predicted
before it is played, and a played session is a measurement against that
prediction, never a reason on its own to move a number.

## Running a sweep

`just balance` runs every preset matchup twenty times on the shipped
tables and prints a summary row per matchup on stderr and a row per bout on
stdout. A sweep varies a table value across the same matchups:

```
just balance duel --matchup carbine-vs-rusher --vary weapons.Carbine.damage=20,24,28
just balance duel --vary weapons.Pistol.interval=0.12,0.15,0.18 --vary actions.Light.duration=0.35,0.40 --out bouts.csv --summary-out summary.csv
just balance list
```

The varied values are leading columns, so a spreadsheet or Python
(`csv.DictReader`) groups the rows by them; `--out` and `--summary-out`
name the two tables' files, and without them the bouts go to stdout and
the summary to stderr, so a bare redirect captures only the bouts. A bout
is a function of its seed and the tables, so two sweeps of the same cell
are the same rows, and a difference between two cells is the table change
and nothing else. The tables the sweep varies are `assets/combat/*.ron`;
they are compiled into `veyra-sim` as well as read from disk, so an
edit there rebuilds the sim, the physics and the harness before the run
(about a minute in release), and the game too.

The game's opponents are in the sweep: the sentry and the rusher presets
run `veyra_sim::combat_ai`, the AI `game --combat` runs, on the
installed `opponents.ron` row, so `opponents.sentry.charge=0.1,0.65`
moves the harness's sentry as it would move the game's.

What the harness answers today, of section 7's list: time to kill,
damage taken, stamina and posture margins, how often a chain lands, the
pistol against the carbine, a master against a novice and a box's
marginal effect (below). The progression model and the household model,
the hours and the income, are still paper.

## Sweeping boxes

A side of a matchup can be on a sheet: `--boxes` (side `a`) and
`--b-boxes` (side `b`) take a list of box ids from
`assets/progression/trees.ron`, `blade:cuts-1,blade:footwork-2` or
`blade.cuts.1,blade.footwork.2`, and a `Side` in a grid spec takes the
same list as `boxes`. The bout fits the sheet to the combatant at spawn
through the merge the game runs (`veyra_sim::progression::apply`), so
a boxed side has the fitted rows and pools and may do only what its boxes
open; a side with no boxes is unfitted and free to do everything, which
keeps every older preset and band as it was. A list that is no sheet (a
box without its step below, an unknown id) is refused before a bout runs.

```
just balance sweep --matchup blade-vs-blade
just balance sweep --matchup carbine-vs-carbine --boxes <moveset>,marksman.carbine.2,marksman.carbine.3
just balance duel --matchup blade-master-vs-novice --vary actions.Heavy.damage=40,50
```

`sweep` is the question "what is this box worth": side `a` on a
baseline (default: the sheet that opens the whole moveset, the two
novices, Cuts 1 to 3 and Carbine 1, 26 points), then every box that
baseline could learn next, from the trees with effects, added alone, one
bout set each, printed as a Markdown table of win rate, time to kill and
damage taken with the change from the baseline, ready for the ledger.
Side `b` is on the same baseline when it is a player, so the box is the
only difference between the sides, and on its own sheet when it is one
of the game's opponents. A deeper box is reached by putting its branch
below it in the baseline. The placeholder trees have no effects and are
not swept.

A box whose row shows no change is not necessarily worthless: the fair
duels on today's tables end in five cuts or six hits, before a reload, a
regen rate or a second Heavy can matter, so a box's worth has to be read
against a matchup long enough to show it; the first sweep in the ledger
says which boxes that was true of.

## Reading the bands

`just balance-check` runs the feel contracts: every band of
[`assets/combat/contracts.ron`](../../assets/combat/contracts.ron), each a
preset matchup, a metric of its summary and a range, over a fixed set of
seeds, and prints a table with what it measured and whether it held. The
same check is the `contracts` test of `tools/balance`, which `just test`
and so `just ci` run, so a table change that breaks a chosen contract
fails CI the way a slower frame fails the perf gates; `just balance-check`
is the release run for a person, not a second gate.

A band comes in two kinds, and its `chosen` flag says which. A chosen
band is the design's range and a miss fails the check: the change moved a
number the design cares about. A band marked "measured, not yet chosen"
(`chosen: false`) is held at what the shipped tables gave on the day it
was written, because they do not yet meet the design; its rationale says
what the design wants, or that a range is proposed and not yet in the
design. A miss there is printed and does not fail, since a motor or a
physics change may legitimately move a number nobody chose; it is still a
change worth a ledger line, and if it was the change that was wanted the
band moves with the tables in the same commit.

A band holds bit for bit on one machine: a bout is deterministic per
seed, but the physics is `f32` through `glam` and avian, whose results
can differ with the target features a build was made with. A band that
fails on another machine and holds here is first a float-determinism
question, to be settled by comparing the bout rows of one seed, not a
table question.

A ratio band (`Relative`) reads its metric against a control matchup, the
same thing without the change being measured, so the number is the worth
of that one thing. A control that measures zero makes the ratio undefined
and the band fails with that said; the dodge band starts its pair at 12 m
for that reason, since at the encounter's 27 m the sentry's bolt misses
a strafing player with or without a dodge.

## Adding a band

Pick or add the preset matchup the number is read from (`tools/balance`'s
`presets()`; `balance list` prints them), choose the metric, and add a
`Band` to `contracts.ron` with a rationale that says why the range is
where it is. Run `just balance-check`. If the shipped tables already meet
the design's range, the band is `chosen: true`; if not, set the range to
what was measured, `chosen: false`, mark it "measured, not yet chosen" in
its comment, and keep the design's wish in the rationale. The tables are not moved to fit a new
band: that is a tuning pass, with its own ledger line.

## Writing a ledger line

Every table change gets a section in [`ledger.md`](ledger.md), appended
and never edited afterwards, in the order the changes land: the date, what
changed and why, the band table `just balance-check` printed after the
change (the prediction), a table for what the next played session
measured, left blank until it has been played, and notes. The played
column is filled in by the person who played, against the same bands,
from the game's `--combat-trace` or a scripted replay (`Scripted` policy)
of the session's inputs. Where play disagrees with the prediction the
model is corrected first, since a table tuned against a wrong model is
tuned to the wrong game, and that correction is a ledger line too.

## The first hour

`just balance field` works a new household's first hour from the shipped data
and the shipped world and prints what it does at which minute; the bands it is
held to are in `assets/balance/first_hour.ron` and the `first_hour` test of
`tools/balance`. What it is, what was moved and what the Surveyor's boxes and a
made carbine are worth is in [first-hour.md](first-hour.md).

## Range presets

`pistol-vs-dummy-10m`, `-30m`, `-60m` and the same three for the carbine put a
shooter that stands and aims in front of the passive dummy, tapping the trigger
at the row's own pace (a press a little before the weapon is ready is latched,
as a hand's would be). `carbine-hold-vs-dummy-8m` and `-30m` hold the trigger
down and `carbine-burst-vs-dummy-8m` is their control. Their five bands say the
carbine wins close, the pistol from mid range on, and that full auto wins at
arm's length and loses at distance (`assets/combat/contracts.ron`).

The rifle's `rifle-vs-dummy-{10,30,60,120}m` stand and aim (and wait for the
aim to close) and `rifle-hip-vs-dummy-30m` fires unaimed on the move
(`HoldAndFire { aim: false }`). Their four bands say it owns the range past
50 m, holds it to 120 m, is the worst of the three at 10 m and is tight only
when aimed.

## Guard presets

`guard-vs-sentry-12m` and `parry-vs-sentry-12m` stand a blade behind its guard
(it never cuts, as the dodge pair neither fires nor strafes) against the same
bracing control as the dodge; `blade-guard-vs-rusher`, `blade-parry-vs-rusher`
and `greatblade-guard-vs-rusher` set a blocking and a parrying `GuardAndCut`
beside `blade-vs-rusher`, and `blade-guard-vs-carbine-25m` has its own control
`blade-vs-carbine-25m`. The chosen bands say a guard takes at most 0.75 of the
bracing control's damage, a parrying blade wins against the rusher and takes
less than the blocking one; the carbine pair is measured, not chosen. The
parrying policy raises no sooner than `REACTION.0` after a blow shows.

## Ability presets

A side may be given abilities by id (`--abilities a,b`, `--b-abilities`, or
`abilities` in a preset); they are known and slotted in order through the
sim's own `change`, and `PressAbilities` presses each whenever `can_use`
allows and its condition holds (in reach for a strike, a clear line for a
shot, hurt for a recharge, a quarter magazine for a reload; a status when it
can help: focused for a loose shot, fortified before a cut in reach, reflecting
at a guard facing bolts. A melee hand waits for a raised guard to drop before a
Hit or Strike and presses nothing that would leave it short of its next cut's
stamina, a bearer of a tight-cone status fires on the move, and an instant
press does not take the tick's pull or cut away). The
Abilities table of the worth page reads each ability on the fight it is for;
an ability never pressed in its fight is worded unread. `precise-pistol-vs-dummy-30m` and `burst-carbine-vs-dummy-30m`
sit beside their controls and hold the two chosen bands; the butt strike is
read on the punishing carbine against the bulwark. The book counts `Abilities` used. The
second half's presets (`suppress-`, `disarm-`, `charged-`, `break-`, `sweep-`,
`reflect-`, `thrust-` and `vault-`) sit beside their controls and are measured
only; the closers are read from 6 m by `FirstCut`, the bout second of the
first cut landed.

`CloseAndCut` and `GuardAndCut` take `sprint` (default false): the side
sprints while farther than twice its reach. The `kit-*` presets (25 m and
12 m) give a sprinting side its trained abilities; the `*-2m` presets start in reach.

## Opponent presets

`Policy::Marksman` and `Policy::Duelist` run the game's own `OpponentBrain`
from the `marksman` and `duelist` rows of `opponents.ron` (sheets fitted as the
game fits them; vary paths `opponents.marksman.*` and `opponents.duelist.*`).
`Policy::Close { within, then }` sprints at the target and then plays `then`,
for "close to 20 m, then fire". The `*-vs-marksman-60m` and `*-vs-duelist-*`
presets are measured in `docs/tickets/0010-weapons-and-active-skills.md`, step
8; several end before the opponent acts, so read that note before choosing a
band on one.

## The worth presets (step 8, second half)

Every weapon against every opponent the game fields, each with its plain
policy, named `worth-<weapon>-vs-<opponent>` for the weapons pistol, carbine,
rifle, blade, greatblade and glaive (`balance::WORTH_WEAPONS`) and the
opponents sentry (27 m), rusher (18 m), marksman (60 m) and duelist (15 m). A
gun fires as it does in the older presets; the rifle is a `StopAndShoot` hand
(walks across the line after a shot, holds the aim closed a beat, fires); a
melee class sprints in with the abilities its branch teaches. The sentry and
rusher sides are fitted from `assets/progression/opponents.ron` as the game
fits them, like the marksman and the duelist.

## The bulwark presets (step 9, second part)

`Policy::Bulwark` is the game's own brain on the `bulwark` row of
`opponents.ron`, fitted from its sheet and given its guard by
`Combatant::with_guard`, as the game does (vary paths
`opponents.bulwark.<field>`, `.guard.<field>`, `.guard.bolts.<field>`).
`Policy::Punish { range }` is the gun's answer: it holds fire while its
target's guard is up, backs off to `range`, dodges a cut it sees begin once, and
fires while the guard is down. The presets are `standing-carbine-`,
`punish-<gun>-`, `blast-carbine-`, `worth-<weapon>-` and
`<class>-light|heavy|guard-vs-bulwark`; `behind-carbine` is left out (a third
actor). Its fights are near-deterministic, so one bout is most of them.

The bulwark's own rules are three figures of its row: `initial_cooldown` is
counted from its arrival in reach, `pinned` is how long a cut on its guard
holds its own cut back, and `stagger_cooldown` is also how long a broken guard
stays down. `blade-light-vs-bulwark` is the preset that reads them: a hand
that only keeps cutting, which the two `a-pressing-blade-` bands hold against
the punishing carbine.

## The seeded cutting hand (step 9)

A bout between a cutting policy and an opponent differs with its seed, as a
gun's does. The opponents' brains draw nothing; the spread is all in the
hand, four figures of `tools/balance/src/policy.rs` (a hand's, not a
table's, so no grid varies them): `ARRIVAL` (0 to 0.25 s, a wait once, on
first reaching the target), `PAUSE_SPREAD` (0.85 to 1.35, a factor on each
pause after an opening), `STEP_SPREAD` (0.9 to 1.1, a factor on the reach it
closes to, never past the weapon's real reach, so a draw over one reads as
one) and `CHAIN_LAG` (0 to 0.08 s, the lag of a press, on a Light's return
cut and on every opening, so a pause of nought still has it). A side that
presses abilities (`PressAbilities`) shares the hand's one clock: a cut,
strike or hit press waits on the hand and sets its next pause. The draws come
from the driver's generator after every draw a gun's driver makes, so a gun
preset reads as it did. A melee win share is therefore a share, and a band on
one may carry its own `bouts` (the file's by default) and its own `cap`
(seconds after which it counts a bout as a draw; a preset runs to the longest
cap any band reading it asks).

A few presets stay nearly one bout because something else sets their clock.
`worth-blade-vs-duelist` thrusts in from a sprint and then its three cuts
wait on the stamina floor, so the stamina sets every opening and swallows
a lag of 0 to 0.08 s: it ends at the same moment and takes 62.0 in every
bout. That is a property of the fight, not of the seeding.

On this machine every run of the harness passes `--jobs 8` (and cargo
`CARGO_BUILD_JOBS=8`), one heavy process at a time, or the CPU fails.

`balance::ability_cases()` is the nineteen abilities each beside its control
and the number it is for (damage a second, damage taken where smaller is
better, the win share for a closer); the contracts' `ability-<id>` bands are
written from it, one a band at 1.03 to 1.60 of the control (0.625 to 0.97 where
smaller is better). A closer whose control wins nothing has no ratio and is
marked in the band's rationale. Guard boxes: `guard-boxes-{trained,untrained}-
{blade,greatblade}-vs-{rusher,duelist}`. Guns inside the duelist's wary
distance: `{pistol,carbine,rifle}-vs-duelist-6m`. The middle: `pistol-vs-rifle-30m`
and `rifle-vs-pistol-80m`. `assets/balance/combat_tuning.ron` is the log of
every dial the pass moved.

`just balance worth [--bouts n] [--write docs/balance/combat-worth.md]` runs
the presets above (and the dummy and rusher presets of the guns and the
classes), reads the tables for the figures a bout cannot give (a class's time
to stagger, what a guard lets through) and writes the page `combat-worth.md`:
guns by range, melee, one row an ability, every weapon against every opponent
and the tuning log. The sentence under each table is written by the command
from its figures. At 60 bouts it takes about four minutes in release. Make it
again after a change to a combat table; the `worth` test of this crate holds
only its shape (a row for each weapon, ability and opponent kind).

