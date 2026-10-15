# Balance ledger

Each combat tuning change, what the harness predicted for it, and what the
next played session measured, appended by hand and never edited afterwards.
The format follows [the perf ledger](../perf/ledger.md); the procedure is
[the README](README.md). The predicted rows are `just balance-check` after
the change (twenty bouts per matchup, seeds 1 to 20, a 30 s cap, release); a measured
row is a played session read against the same bands, blank until one has
been played since the change.

## Baseline

2026-10-01. No tuning; the shipped tables are the numbers that were in
`Action::spec`, `Weapon::spec`, the game's encounter and the player's
controller before they moved to `assets/combat/*.ron`, and the harness
and the bands exist from today. Three bands are held at what was
measured, not where the design wants them, and are not chosen; the
notes say what is wanted and what is only proposed.

Predicted:

| Band | Matchup | Metric | Low | High | Measured | Held |
| --- | --- | --- | --- | --- | --- | --- |
| fair-duel-length | carbine-vs-carbine | mean_time | 1.40 | 2.20 | 1.75 | yes |
| pistol-rate-share | pistol-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.85 | 1.05 | 0.95 | yes |
| blade-closes-on-carbine | blade-vs-carbine-12m | a_win_rate | 0.00 | 0.10 | 0.00 | yes |
| dodge-earns-its-stamina | dodge-vs-sentry-12m | a_taken / brace-vs-sentry-12m | 0.00 | 0.70 | 0.02 | yes |
| chain-lands-on-a-standing-target | chain-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |

Measured in play:

| Band | Session | Measured | Held |
| --- | --- | --- | --- |
| fair-duel-length | | | |
| pistol-rate-share | | | |
| blade-closes-on-carbine | | | |
| dodge-earns-its-stamina | | | |
| chain-lands-on-a-standing-target | | | |

Notes:

- **fair-duel-length**, measured, not yet chosen. The design wants 8 to
  14 s. Two carbines at 12 m with an exact lead on open ground put six
  hits each into 140 pools in 1.75 s; one bout in twenty was a draw with
  both falling on the same tick. The game's encounter lasts longer because
  of cover, two hostiles and a script that strafes, none of which a flat
  floor has; whether the duel band is met by the tables (damage, pools,
  spread) or by the arena is the first tuning question.
- **pistol-rate-share**, measured, not yet chosen. A range of 0.70 to
  0.90 is proposed here; the design states none. The carbine (24 per 0.24 s) and the pistol (15 per 0.15 s) have
  the same nominal rate, and only the pistol's reload after nine shots
  separates them against a 140-pool dummy: 0.95.
- **blade-closes-on-carbine**, measured, not yet chosen. A coin toss,
  0.35 to 0.65, is proposed here; the design states none. The blade walks
  12 m in about 2.2 s at `actor.walk_speed` and the carbine kills in about
  1.6, so the blade never arrives: 0.00 in twenty bouts.
- **dodge-earns-its-stamina** holds at the design's value, read at 12 m
  with the game's own sentry (`strata_sim::combat_ai`) against a player
  who braces and does not fire: the sentry's charge is aimed where the
  player stood when it began, so a player who stands there takes every
  bolt (twelve in 27.6 s, 140 pools) and one who dodges 0.15 to 0.45 s
  into the charge takes none in the 30 s cap (19 dodges at 26 stamina
  each run the pool down to 6; past the cap a charge that finds it empty
  lands). The player's gun is out of the pair on purpose: a player who
  fires kills the sentry before its first charge ends, and a strafing
  control is never hit, so either pair measures nothing against nothing.
- **chain-lands-on-a-standing-target** holds: Light, its return cut and a
  Heavy all land on the dummy, which breaks posture once per bout.
- The summary rows of the other presets on the same day are in the
  harness report (`carbine-vs-rusher` 1.59 s, `carbine-vs-sentry` 1.83 s
  with one sentry shot, `pistol-vs-rusher` 1.49 s, `kite-vs-rusher`
  1.55 s, `kite-vs-sentry` 2.18 s, `blade-vs-carbine` 0.00 win rate at
  15 m, `blade-vs-blade` 2.10 s); a rusher walking straight in dies to a
  carbine in about 1.5 s, and the game's sentry, which waits 1.0 s and
  charges 0.65 before its first shot, dies to one in 1.8.

## Boxes in the harness

2026-10-02. No table change. The progression slice landed: the trees
(`assets/progression/trees.ron`), the sheet, the merge
(`strata_sim::progression::apply`), the household and the screens. The
harness grew a dimension for it: a side may carry boxes, fitted at
spawn through the game's merge, and `balance sweep` reads every box's
marginal effect against a baseline. Two bands were added, both chosen,
since the shipped tables meet them; every older band is unchanged and
its matchups are unfitted, so their numbers are the same as the
baseline's. The dodge pair reads 0.00 where the baseline line read
0.02: the bracing control still takes every bolt, and the dodging player
now takes none in the 30 s cap.

Predicted:

| Band | Matchup | Metric | Low | High | Measured | Held |
| --- | --- | --- | --- | --- | --- | --- |
| fair-duel-length | carbine-vs-carbine | mean_time | 1.40 | 2.20 | 1.75 | yes |
| pistol-rate-share | pistol-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.85 | 1.05 | 0.95 | yes |
| blade-closes-on-carbine | blade-vs-carbine-12m | a_win_rate | 0.00 | 0.10 | 0.00 | yes |
| dodge-earns-its-stamina | dodge-vs-sentry-12m | a_taken / brace-vs-sentry-12m | 0.00 | 0.70 | 0.00 | yes |
| chain-lands-on-a-standing-target | chain-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| cuts-3-lands-the-heavy | heavy-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| blade-master-beats-novice | blade-master-vs-novice | a_win_rate | 0.80 | 1.00 | 1.00 | yes |

Measured in play:

| Band | Session | Measured | Held |
| --- | --- | --- | --- |
| cuts-3-lands-the-heavy | | | |
| blade-master-beats-novice | | | |

Box sweep, `just balance sweep --matchup blade-vs-blade --matchup
carbine-vs-carbine` (twenty bouts a row, seeds 1 to 20, a 60 s cap,
release). Both sides on the moveset sheet (`blade.novice`, Cuts 1 to 3,
`marksman.novice`, Carbine 1, 26 points); each row adds one box side `a`
could learn next:

`blade-vs-blade`, 8 m:

| Box | Cost | Win rate | Change | Time to kill | Change | Taken | Change |
| --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | | 1.00 | | 2.10 | | 68.0 | |
| marksman.carbine.2 | 4 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| marksman.pistol.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| marksman.steadiness.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| marksman.field_craft.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| blade.cuts.4 | 8 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| blade.guard.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| blade.footwork.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |
| blade.breaking.1 | 2 | 1.00 | 0.00 | 2.10 | 0.00 | 68.0 | 0.0 |

`carbine-vs-carbine`, 12 m:

| Box | Cost | Win rate | Change | Time to kill | Change | Taken | Change |
| --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | | 0.40 | | 1.75 | | 130.8 | |
| marksman.carbine.2 | 4 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| marksman.pistol.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| marksman.steadiness.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| marksman.field_craft.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.cuts.4 | 8 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.guard.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.footwork.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.breaking.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |

The same carbine duel one step deeper, both sides on the moveset plus
Carbine 2, Steadiness 1, Field craft 1 and 2, so the third step of each
branch is a row:

| Box | Cost | Win rate | Change | Time to kill | Change | Taken | Change |
| --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | | 0.40 | | 1.75 | | 130.8 | |
| marksman.carbine.3 | 6 | 0.90 | +0.50 | 1.62 | -0.14 | 113.6 | -17.2 |
| marksman.pistol.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| marksman.steadiness.2 | 4 | 0.40 | 0.00 | 1.75 | -0.01 | 130.8 | 0.0 |
| marksman.field_craft.3 | 6 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.cuts.4 | 8 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.guard.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.footwork.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |
| blade.breaking.1 | 2 | 0.40 | 0.00 | 1.75 | 0.00 | 130.8 | 0.0 |

Notes:

- **cuts-3-lands-the-heavy** holds and is chosen: the Cuts 3 sheet
  swinging only Heavies at the dummy lands every one. Without the box
  the sim drops every Heavy, no cut begins and the share is 0.00 (checked
  with `duel --matchup heavy-vs-dummy --boxes blade:novice,blade:cuts-1,blade:cuts-2`:
  twenty draws, zero cuts), so the band reads the unlock and not the
  motor.
- **blade-master-beats-novice** holds and is chosen at twenty bouts in
  twenty. The novice here is the rusher's sheet (the novice box and Cuts
  1 to 3), not the novice box alone, so both sides chain and the
  fourteen other boxes are what is measured; a sheet with the novice box
  alone cannot chain and would make the band trivial.
- **The first-step sweep moves nothing.** On today's tables a fair duel
  is over in five cuts (2.10 s) or six hits (1.75 s), before a reload
  (Carbine 2), a regen rate (Guard 1), a cheaper dodge (Footwork 1) or
  the extra posture of a Light (Breaking 1) can tell, and Cuts 4's 15
  percent on the Heavy does not take a cut off the five. This is the
  `fair-duel-length` complaint from the other side: until the duel is
  long enough to answer in, no box of the first step is worth its
  points in it, and the sweep should be rerun after the tables are
  tuned to the 8 to 14 s band before any box cost is moved.
- **Carbine 3 is the exception**: the interval from 0.24 to 0.20 s wins
  the fair duel in nine bouts of ten from a baseline of four, 0.14 s
  sooner and 17 pools less taken, for 6 points. One box of the third
  step deciding a fair duel is the kind of number the sweep exists to
  find; whether it is the box or the duel's shortness is the same
  question as above.
- **The blade mirror is not a fair duel.** Side `a` wins every seed
  with the same row (five cuts, 68 taken, side `b` broken on the third):
  the policies open without a reaction on a cut, so the side that spawns
  at the origin opens first and the mirror is decided by that. A box
  sweep on it reads nothing until the blade policy has the reaction the
  guns have; its rows above are kept because the request named the
  matchup, not because they say anything.

## 2026-10-11, the base made rough (ticket 0007, step 11)

The user: "I'm ok making the base weapons quite a bit worse, will reward
crafting/optimization". `weapons.ron` version 3: the carbine's cones 2.4 and
0.9 degrees, the pistol's 2.8 and 1.3. The reflex sight's AimSpread factor
0.94 to 0.70. `fitting.ron`'s bands: benefit 0.4 + 1.2q, drawback 1.5 - 1.0q.
The patrol carbine's `interval` line is its signature: 1.30 of the table's
interval at 0 to 0.90 at 1000, so a bought one fires at 0.264 s. Three
chosen: false bands re-centred, four added (bought, made, sight at 55 m,
fine sight). The four chosen: true bands are unedited.

Predicted:

| Band | Matchup | Metric | Low | High | Measured | Held |
| --- | --- | --- | --- | --- | --- | --- |
| fair-duel-length | carbine-vs-carbine | mean_time | 1.40 | 2.10 | 1.75 | yes |
| pistol-rate-share | pistol-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.76 | 1.14 | 0.95 | yes |
| a-bought-carbine-is-slower | bought-carbine-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.75 | 0.98 | 0.94 | yes |
| a-made-carbine-is-faster | made-carbine-vs-dummy | a_dealt_per_second / bought-carbine-vs-dummy | 1.02 | 1.40 | 1.12 | yes |
| a-sight-tightens-aimed-fire | sighted-carbine-vs-dummy-55m | a_dealt_per_second / aimed-carbine-vs-dummy-55m | 1.05 | 1.60 | 1.29 | yes |
| a-fine-sight-beats-a-store-sight | fine-sighted-carbine-vs-dummy-55m | a_dealt_per_second / sighted-carbine-vs-dummy-55m | 1.01 | 1.30 | 1.08 | yes |
| blade-closes-on-carbine | blade-vs-carbine-12m | a_win_rate | 0.00 | 0.10 | 0.00 | yes |
| dodge-earns-its-stamina | dodge-vs-sentry-12m | a_taken / brace-vs-sentry-12m | 0.00 | 0.70 | 0.00 | yes |
| chain-lands-on-a-standing-target | chain-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| cuts-3-lands-the-heavy | heavy-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| blade-master-beats-novice | blade-master-vs-novice | a_win_rate | 0.80 | 1.00 | 1.00 | yes |

Measured: blank until played. The 1.12 of the made carbine is a harness
figure: a line of 900 is not yet makeable (the shipped ceiling is 700).

## 2026-10-11, a harness made, the sight seen (ticket 0007, step 11 follow-up)

`weapons.ron` version 4: the carbine's cones 4.0 and 2.0 degrees, the
pistol's 4.4 and 2.4, so the reflex sight shows at the encounter's ranges
(carbine-vs-sentry 2.13 s with the store sight against 2.63 s without). A
harness is made by `veyra:schematic.harness`, so the made carbine preset reads
an `interval` line of 800 (a figure a smith can make) where it read 900. The
sight trio moved from 55 m to the sentry's 27 m, where the unsighted bolts
land 0.49 of the time (0.76 with the store sight, 0.90 with the 900 one).
The four chosen: true bands are unedited. Bands re-centred a fifth either
side of what was measured, a bound kept on the near side of 1.00 where a part
that does nothing reads exactly 1.00.

| Band | Matchup | Metric | Low | High | Measured | Held |
| --- | --- | --- | --- | --- | --- | --- |
| fair-duel-length | carbine-vs-carbine | mean_time | 1.40 | 2.10 | 1.76 | yes |
| pistol-rate-share | pistol-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.70 | 1.06 | 0.88 | yes |
| a-bought-carbine-is-slower | bought-carbine-vs-dummy | a_dealt_per_second / carbine-vs-dummy | 0.75 | 0.98 | 0.94 | yes |
| a-made-carbine-is-faster | made-carbine-vs-dummy | a_dealt_per_second / bought-carbine-vs-dummy | 1.02 | 1.31 | 1.09 | yes |
| a-sight-tightens-aimed-fire | sighted-carbine-vs-dummy-27m | a_dealt_per_second / aimed-carbine-vs-dummy-27m | 1.15 | 1.73 | 1.44 | yes |
| a-fine-sight-beats-a-store-sight | fine-sighted-carbine-vs-dummy-27m | a_dealt_per_second / sighted-carbine-vs-dummy-27m | 1.01 | 1.32 | 1.10 | yes |
| blade-closes-on-carbine | blade-vs-carbine-12m | a_win_rate | 0.00 | 0.10 | 0.00 | yes |
| dodge-earns-its-stamina | dodge-vs-sentry-12m | a_taken / brace-vs-sentry-12m | 0.00 | 0.70 | 0.00 | yes |
| chain-lands-on-a-standing-target | chain-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| cuts-3-lands-the-heavy | heavy-vs-dummy | a_cut_share | 0.90 | 1.00 | 1.00 | yes |
| blade-master-beats-novice | blade-master-vs-novice | a_win_rate | 0.80 | 1.00 | 1.00 | yes |
