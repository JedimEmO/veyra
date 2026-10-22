# balance

The duel harness of `docs/design/progression.md` section 7. It runs bouts
between policies over the real combat: `veyra-sim`'s `CombatPlugin` (the
action clocks, stamina, posture, magazines), `strata-physics`'s
`resolve_attacks`, bolt flight, blade sweeps and character motor, in a
headless Bevy app on a flat floor, stepped at exactly 60 Hz with no wall
clock, so a bout runs as fast as the CPU goes. Nothing here computes a
number the game computes; the policies write inputs and the recorder
watches. A bout with the same seed and tables gives the same CSV row.

```
just balance                                   every preset, 20 bouts each
cargo run --release -p balance -- duel --matchup carbine-vs-rusher --bouts 20
cargo run --release -p balance -- duel --vary weapons.Pistol.interval=0.12,0.15,0.18 \
                                       --vary actions.Light.duration=0.35,0.40
cargo run --release -p balance -- duel --out bouts.csv          summary to stdout
cargo run --release -p balance -- grid --spec grid.ron
cargo run --release -p balance -- duel --matchup blade-vs-blade --boxes blade:cuts-1,blade:cuts-2 \
                                       --b-boxes blade:novice           sides on sheets
cargo run --release -p balance -- sweep --matchup blade-vs-blade       every box's marginal effect
cargo run --release -p balance -- duel --matchup carbine-vs-dummy \
                                       --fit veyra:module.patrol_extended_feed   a module on side a's weapon
cargo run --release -p balance -- list                          matchups and paths
just balance-check                             the feel contracts, assets/combat/contracts.ron
```

## Output

Two CSV tables. One row per bout, with the varied values as leading
columns named by their paths:

```
<paths...>,matchup,seed,winner,time,
a_dealt,a_taken,a_shots,a_hits,a_cuts,a_cuts_landed,a_stamina_min,a_posture_min,a_breaks,a_dodges,a_dodges_avoided,
b_dealt,... the same for b
```

`winner` is `a`, `b` or `draw` (the time cap, `--cap`, default 60 s);
`time` is seconds until the last of the losing side fell; `dealt` and
`taken` are health plus shield; `cuts` counts every Light, return cut and
Heavy begun; `stamina_min` and `posture_min` are the lowest either pool
reached, so a chain that nearly broke posture reads apart from one that
never threatened it; `breaks` are posture breaks suffered;
`dodges_avoided` is approximate: dodges during which an enemy bolt passed
within a metre of the capsule or an enemy blade's contact window was open
within 2.6 m, read by the recorder from positions, not by the physics'
sweep, and a bolt dodged on the charge arrives after the dodge has ended
and is not counted. It is a column to read, not one a contract is on.

One summary row per cell and matchup: the varied values, `matchup`,
`bouts`, `a_win_rate`, `draw_rate`, `mean_time` over the decided bouts,
then the mean of every per-side column.

By default the bout rows go to stdout and the summary to stderr, so a pipe
gets one table. With `--out FILE` the bouts go to the file and the summary
to stdout; `--summary-out FILE` names its own file. The last line on stderr
is the timing: bouts, wall seconds, milliseconds per bout, jobs.

## Matchups

A matchup is two sides and the metres between them. A side is a policy, a
weapon in hand and a profile: `Player` (`actor.ron`'s pools and motor),
`Rusher` or `Sentry` (`opponents.ron`'s pools, damage scale, walk speed
and the sentry's slow bolt, which the game gives every opponent). The
presets:

| name | a | b | start |
| --- | --- | --- | --- |
| `carbine-vs-rusher` | player, carbine, HoldAndFire 12 m | the game's rusher | 18 m |
| `carbine-vs-sentry` | the same | the game's sentry | 27 m |
| `pistol-vs-rusher` | player, pistol, HoldAndFire 12 m | rusher | 18 m |
| `kite-vs-rusher` | player, carbine, Kite 10 m with dodges | rusher | 18 m |
| `kite-vs-sentry` | the same | sentry | 27 m |
| `blade-vs-carbine` | player, pistol, CloseAndCut chained, a Heavy every third | player, carbine, HoldAndFire 12 m | 15 m |
| `blade-vs-blade` | the same | the same | 8 m |
| `carbine-vs-carbine` | player, carbine, HoldAndFire 12 m | the same | 12 m |
| `blade-vs-carbine-12m` | the blade | player, carbine, HoldAndFire 12 m | 12 m |
| `dodge-vs-sentry-12m` | player, Brace with dodges | sentry | 12 m |
| `brace-vs-sentry-12m` | player, Brace without | sentry | 12 m |
| `carbine-vs-dummy` | player, carbine, HoldAndFire 12 m | player, Stand | 12 m |
| `bought-carbine-vs-dummy` | the same on a bought carbine's record (every line 500: 1.10 of the table's interval) | player, Stand | 12 m |
| `made-carbine-vs-dummy` | the same with its `interval` line at 800 (a figure a smith can make) | player, Stand | 12 m |
| `aimed-carbine-vs-dummy-27m` | bought carbine, HoldAndFire 27 m (aiming, trigger held) | player, Stand | 27 m |
| `sighted-carbine-vs-dummy-27m` | the same with the reflex sight at store grade | player, Stand | 27 m |
| `fine-sighted-carbine-vs-dummy-27m` | the same with the sight's lines at 900 | player, Stand | 27 m |
| `pistol-vs-dummy` | player, pistol, HoldAndFire 12 m | player, Stand | 12 m |
| `chain-vs-dummy` | player, CloseAndCut chained, a Heavy every second | player, Stand | 4 m |
| `heavy-vs-dummy` | player on the Cuts 3 sheet, CloseAndCut, every opening a Heavy | player, Stand | 4 m |
| `blade-master-vs-novice` | player on the whole Blade tree, the blade | player on the Cuts 3 sheet, the blade | 8 m |

`carbine-vs-rusher` and `carbine-vs-sentry` are the game's encounter
opponents, the shipped AI (`Sentry` and `Rusher` below), against a player
who stands and shoots. The rest are the pairs the feel contracts
read: a fair duel, the blade's closing problem from the design's 12 m,
the dodge against its control with neither side's gun in the number, a
dummy for the two weapons' rates and the chain, and the two boxed pairs
below. The Cuts 3 sheet is `blade.novice` and `blade.cuts.1` to `3`, the
rusher's sheet in `assets/progression/opponents.ron`.

## Boxes

A side may carry `boxes`, a list of box ids from
`assets/progression/trees.ron`, in the RON (`boxes: ["blade.novice",
"blade.cuts.1"]`) or on the command line (`--boxes blade:cuts-1,blade:cuts-2`
for side `a`, `--b-boxes` for `b`; the colon-and-dash spelling is the
same id). At spawn the bout builds the sheet and fits it to the
combatant through `veyra_sim::progression::apply`, the merge the game
runs, so the side's rows, pools, magazine, bolt and motor are the fitted
ones and it may begin only the actions its boxes unlock and fire only the
weapons they certify. A side with no boxes is unfitted and reads the
installed tables live, free to do everything, which is what every preset
without boxes did before the trees existed, so their rows and bands are
unchanged. The list is checked as an authored sheet before any bout
runs: every id exists, every prerequisite is in the list, the budget
holds; XP gates are not checked, since the harness asks what a box is
worth, not whether it has been earned. The graph is `--assets`'s
`progression/trees.ron`, or the compiled-in one.

`sweep --matchup NAME` is the design's "every box's marginal effect":
side `a` on a baseline sheet (`--boxes`; the default is the sheet that
opens the whole moveset, the two novices, Cuts 1 to 3 and Carbine 1,
since a sheet with less cannot chain or fire), then each box that sheet
could learn next, from every tree with effects, added alone, one bout set
per row, and a Markdown table with the win rate, the mean time of the
decided bouts and side `a`'s damage taken, each with its change from the
baseline. Side `b` takes `--b-boxes`, or `a`'s baseline when it is a
player (so the box under test is the only difference between the sides),
or stays unfitted when it is one of the game's opponents, whose sheet is
its own. To reach a branch's upper boxes, put the lower ones in the
baseline: `--boxes <the moveset>,blade.guard.1,blade.guard.2` lists
Guard 3 among the rows.

## Modules

A side may carry `fitted`, the modules on its weapons, in the RON
(`fitted: [(weapon: Carbine, module: (item:
"veyra:module.patrol_extended_feed", lines: {"capacity": 500,
"reload_time": 500}))]`) or on the command line: `--fit` for side `a` and
`--b-fit` for `b`, on `duel` and `sweep`, repeatable. The spelling is an
item id of `assets/items/`, then any of its quality lines after an `@`:
`--fit veyra:module.patrol_extended_feed@capacity=610,reload_time=433`. A
line left out is store grade, 500, so the bare id is the bought module.
`--line interval=900` (and `--b-line`) sets one of the weapon's own lines on
its record, the carbine's signature, so the shooter holds a made carbine; a
side with `--line` or a `bought-` preset reads a bought record at 500.

At spawn, after the boxes, the bout fits each module to a bought example
of the weapon it is made for and equips the combatant with the loadout
`veyra_sim::fitting` makes of them, which is what the game does with
the weapons in a player's hands: the tables' row, then the boxes, then
each module's factor once, and the magazines start full at their fitted
size. Only what the fight has a number for reaches the row (capacity,
reload, interval, draw, aimed pace, damage, posture, cut stamina); a
sight's spread is carried in the data and changes nothing here. A side
with nothing fitted is not equipped at all and reads its rows as every
side did before, so the older rows and bands keep their numbers. The
modules are checked before any bout runs: an unknown id, a line the
module has not got, a mount a bought weapon has shut, or two for one
mount is refused by name. The items are `--assets`'s.

`tests/loadout.rs` is the measure of the first one: the patrol magazine
fires 27 rounds in 6.5 s where the bare carbine fires 24 and is
reloading, and by 8.1 s the two are level at 29, the longer reload
having paid the five rounds back.

## Policies

Deterministic input writers with a seed for their small choices (the
strafe direction and when it flips, and a reaction of 0.15 to 0.45 s before
the first trigger pull or opening, which is what spreads a cell's bouts).
They write `PlayerIntent` and `CombatIntent` as the game's input and its
encounter AI do, and nothing else.

- `HoldAndFire { range, cadence }`: holds `range` by backing off under
  0.6 of it and advancing past 1.5 of it or when the muzzle has no line,
  strafes between, fires with a lead on the target's velocity when the
  line is clear, reloads when empty. `cadence` zero holds the trigger; a
  positive one taps it, pulled the tick the weapon can take a press and let
  go the next, so a pistol fires a shot a press and a carbine a burst a
  press at the weapon's own pace (the presets pass the row's pace as the
  nominal figure; nothing waits on it).
- `Aim { cadence }`: the same trigger, standing still: aims and fires when
  the line is clear. The range presets (`pistol-vs-dummy-10m`, `-30m`,
  `-60m`, `carbine-vs-dummy-10m`, `-30m`, `-60m`, and the held trigger's
  `carbine-hold-vs-dummy-8m`, `-30m` with `carbine-burst-vs-dummy-8m` as its
  control) use it, so a weapon's reach reads with no footwork in the number.
- `CloseAndCut { chain, heavy_every, cooldown, reach }`: walks to `reach`
  metres and cuts. `chain` queues the return cut behind every Light;
  every `heavy_every`th opening is a Heavy; `cooldown` is the seconds
  between openings. The presets' players cut from `BLADE_RANGE`, 1.7 m,
  the blade curve's farthest sweep point with a hand's slack.
- `Kite { dodge_on_telegraph, range }`: strafes at `range` and fires;
  with the flag it dodges sideways when the opponent's swing begins
  within 4 m, when a charge shows (0.15 to 0.45 s after, once per
  charge) or when a bolt leaves a muzzle with no charge before it.
- `Brace { dodge_on_telegraph }`: stands its ground facing the opponent
  and never fires; with the flag it dodges as `Kite` does. The dodge's
  pair, with the gun and the footwork out of the number.
- `Sentry` and `Rusher`: the game's opponents. They run
  `veyra_sim::combat_ai`, the code `game --combat` runs, on the
  installed `opponents.ron` row every tick: the sentry waits `cooldown`,
  holds between `hold_near` and `hold_far`, charges `charge` seconds with
  its aim locked where the target stood (the telegraph `actor.charge`
  shows), fires, recovers `recover`; the rusher closes to `reach` and
  swings a Heavy every `cooldown`. Nothing of the row is copied here, so
  every `opponents.*` path a grid varies moves them as it moves the game.
- `Stand`: does nothing, the target dummy a rate is read against.
- `Scripted { path }`: replays a CSV of inputs,
  `tick,move_x,move_y,yaw,sprint,fire,aim,light,heavy,dodge,reload,weapon`,
  a row holding until the next and its edges firing once on its tick.

Two numbers are the policies' own and no grid varies them: `REACTION`,
the 0.15 to 0.45 s before a first trigger pull or opening and before a
charge is answered, and `LEAD_CAP`, the 0.6 s of bolt flight a gun's
lead accounts for at most, which is why a slow bolt at the encounter's
27 m misses a strafing player.

## Grids

`--vary path=v1,v2,...` is repeatable and the grid is the product. A path
is a field of the four combat tables: `actions.<Action>.<field>`,
`weapons.<Weapon>.<field>`, `actor.<field>`, `opponents.<field>`,
`opponents.sentry.<field>`, `opponents.rusher.<field>`; `balance list`
prints every one. Each cell is installed process-wide
(`veyra_sim::combat_tables::install`) and its bouts run before the
next, `--jobs` at a time; the base tables are `--assets`'s `combat/`
(default `assets`), or the compiled-in ones.

`grid --spec FILE` takes the same thing as RON, the `balance::Grid` type;
a matchup is a preset's name or one spelled out:

```ron
Grid(
    vary: [("weapons.Pistol.interval", [0.12, 0.15, 0.18])],
    matchups: [
        "pistol-vs-rusher",
        Matchup(
            name: "pistol-vs-two-rushers",
            a: Side(policy: HoldAndFire(range: 12.0), weapon: Pistol, profile: Player),
            b: Side(policy: Rusher, weapon: Carbine, profile: Rusher, count: 2),
            distance: 18.0,
        ),
    ],
    bouts: 20,
    seed: 1,
    cap: 60.0,
)
```

A side's `count` (default 1) fields several of them abreast.

The four tables are compiled into `veyra-sim` as well as read from
`--assets`, so an edit to `assets/combat/*.ron` rebuilds the sim, the
physics and this binary before the run starts, about a minute in release
on a warm cache.

## Contracts

`balance check` (`just balance-check`) runs the bands of
`assets/combat/contracts.ron`: each names a preset, a metric and a range,
and the check runs every preset the bands need once (`bouts` bouts from
`seed`), prints a table with what each band measured and whether it held,
and exits 1 when a chosen band (`chosen: true`) did not; a band that is
only measured, not chosen, prints its miss and does not fail. The metrics
are the summary columns (`WinRate`, `DrawRate`, `MeanTime`, `A(...)` and
`B(...)` over a side's book, with `DealtPerSecond`, `HitShare` and
`CutShare` as ratios per bout) and `Relative(metric, control)`, the
metric over the same metric of a control preset. `tests/contracts.rs` is
the same check as a test and the gate in `just test`;
`docs/balance/README.md` says what to do when a band fails.
