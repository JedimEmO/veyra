# What the weapons and abilities are worth

Made by `just balance worth --write docs/balance/combat-worth.md` from the duel harness over the shipped tables, 60 bouts a preset. Do not edit it: make it again after a change to a table. Every sentence under a table is written by the command from the figures above it.

## Guns by range

Each gun standing and aiming at the passive dummy (the rifle one shot at its own interval). Damage a second over the bout, and the share of shots that struck. The best damage a second at each range is in bold; a gun whose row does not reach a range says so.

| Weapon | Range | Damage a second | Hit share |
|---|---|---|---|
| Pistol | 10 m | 68.8 | 100% |
| Pistol | 30 m | 44.4 | 66% |
| Pistol | 60 m | 8.2 | 22% |
| Pistol | 120 m | out of range (90 m) | - |
| Carbine | 10 m | **110.5** | 100% |
| Carbine | 30 m | 29.5 | 31% |
| Carbine | 60 m | 5.0 | 9% |
| Carbine | 120 m | out of range (70 m) | - |
| Rifle | 10 m | 50.6 | 100% |
| Rifle | 30 m | **48.0** | 100% |
| Rifle | 60 m | **44.7** | 100% |
| Rifle | 120 m | **37.6** | 96% |

Most damage a second: Carbine at 10 m, Rifle at 30 m, Rifle at 60 m, Rifle at 120 m. Never first on the dummy: Pistol.

## Melee

Each class with the cutting policy of its dummy and rusher presets. Damage a second is measured. Time to stagger is worked from the tables: a Light then Followups, until the posture pool is gone (hits, then seconds). The last two columns are what the class's guard lets through of one carbine round and of the table's Heavy: damage, posture and the stamina the guard pays.

| Class | Damage/s on the dummy | Damage/s on the rusher | Time to stagger | Guard, carbine round | Guard, Heavy |
|---|---|---|---|---|---|
| Blade | 38.2 | 32.5 | 3 hits, 0.93 s | 9.6 / 6.0 / 20.2 | 22.0 / 32.0 / 46.2 |
| Greatblade | 32.5 | 30.7 | 2 hits, 0.93 s | 4.8 / 3.0 / 13.4 | 11.0 / 16.0 / 30.8 |
| Glaive | 29.7 | 30.6 | 3 hits, 1.26 s | 12.0 / 7.5 / 20.4 | 27.5 / 40.0 / 46.8 |

Blade and Greatblade stagger soonest (0.93 s) and the Greatblade guard lets the least posture of a Heavy through (16.0).

## Abilities

Each ability beside a control without it, on the weapon and at the range it is for, read on the number it is for (damage a second, the win share for a closer, damage taken, posture left or seconds to win for a defensive one, where a smaller figure is the better). The two seconds columns show what the ability did to the length of the fight, which a gain on another number can hide. The change is the ability's side over the control's, as a gain (inverted where smaller is better). Uses a minute is the lesser of what the cooldown with the commitment and the stamina pool with its regeneration allow. The word: strong over +40 percent, pays +10 to +40, slight +3 to +10, dead under +3; a closer whose control wins nothing is marked opens when it wins something and unread when it wins nothing either.

| Ability | Weapon | Reads | Control | With | Change | Seconds control | Seconds with | Stamina | Cooldown | Uses a minute | Word |
|---|---|---|---|---|---|---|---|---|---|---|---|
| heavy_cut | Blade | damage/s | 32.81 | 38.20 | +16% | 10.4 | 8.9 | 32 | 0.0 s | 48 | pays |
| butt_strike | Carbine | damage taken | 11.67 | 0.00 | no damage taken | 6.3 | 6.7 | 20 | 6.0 s | 10 | strong |
| precision_shot | Pistol | damage/s | 44.45 | 56.61 | +27% | 3.3 | 2.5 | 25 | 8.0 s | 8 | pays |
| aimed_burst | Carbine | damage/s | 29.50 | 37.67 | +28% | 5.3 | 4.3 | 25 | 10.0 s | 6 | pays |
| focus | Rifle | seconds to win | 4.95 | 2.70 | +83% | 4.9 | 2.7 | 30 | 18.0 s | 3 | strong |
| kinetic_absorb | Greatblade | damage taken | 62.61 | 42.62 | +47% | 7.4 | 5.1 | 30 | 12.0 s | 5 | strong |
| rapid_reload | Carbine | damage/s | 30.53 | 32.86 | +8% | 23.4 | 21.7 | 15 | 10.0 s | 6 | slight |
| shield_recharge | Pistol | win share | 0.53 | 0.77 | +44% | 12.7 | 14.8 | 40 | 30.0 s | 2 | strong |
| ricochet | Pistol | damage/s | 36.74 | 41.07 | +12% | 18.7 | 17.1 | 20 | 9.0 s | 7 | pays |
| disarm | Blade | damage taken | 36.60 | 30.42 | +20% | 3.2 | 4.3 | 25 | 7.0 s | 9 | pays |
| suppressing_fire | Carbine | damage/s | 61.10 | 76.97 | +26% | 1.7 | 1.4 | 30 | 14.0 s | 4 | pays |
| explosive_shot | Carbine | seconds to win | 6.27 | 5.66 | +11% | 6.3 | 5.7 | 35 | 10.0 s | 6 | pays |
| charged_shot | Rifle | damage/s | 44.68 | 55.26 | +24% | 3.1 | 2.5 | 35 | 14.0 s | 4 | pays |
| armour_break | Rifle | seconds to win | 4.95 | 3.88 | +27% | 4.9 | 3.9 | 30 | 18.0 s | 3 | pays |
| thrust | Blade | win share | 0.00 | 0.40 | from nothing | 1.1 | 1.7 | 25 | 8.0 s | 8 | opens |
| reflect_shot | Blade | damage/s | 16.36 | 20.97 | +28% | 6.4 | 5.0 | 15 | 9.0 s | 7 | pays |
| sweeping_cut | Glaive | damage/s | 24.56 | 34.05 | +39% | 8.9 | 6.3 | 35 | 6.0 s | 10 | pays |
| vault | Glaive | damage taken | 25.80 | 12.00 | +115% | 5.0 | 4.2 | 30 | 12.0 s | 5 | strong |
| combat_slide | Carbine | damage taken | 22.66 | 14.16 | +60% | 45.8 | 47.9 | 20 | 6.0 s | 10 | strong |

19 abilities: 6 strong, 11 pay, 1 slight, 0 dead, 1 open a fight the control cannot, 0 unread.

## Opponents

Every weapon against each opponent kind with its plain policy: a gun fires as its presets do, a melee class sprints in with its kit on the bar. Win share, mean seconds of the bouts it won, and damage taken. Among the weapons that win at least half their bouts, the quickest is marked fastest and the one that takes least is marked safest.

| Opponent | Weapon | Win share | Seconds to win | Damage taken | Mark |
|---|---|---|---|---|---|
| sentry | Pistol | 100% | 2.2 | 2.4 | fastest |
| sentry | Carbine | 100% | 3.1 | 6.8 |  |
| sentry | Rifle | 100% | 4.7 | 0.2 | safest |
| sentry | Blade | 100% | 4.7 | 12.0 |  |
| sentry | Greatblade | 100% | 6.3 | 24.0 |  |
| sentry | Glaive | 100% | 4.2 | 12.0 |  |
| rusher | Pistol | 100% | 1.8 | 0.0 | safest |
| rusher | Carbine | 100% | 1.6 | 0.0 | fastest, safest |
| rusher | Rifle | 100% | 4.6 | 0.0 | safest |
| rusher | Blade | 100% | 3.0 | 27.5 |  |
| rusher | Greatblade | 100% | 4.1 | 17.4 |  |
| rusher | Glaive | 100% | 2.6 | 27.5 |  |
| marksman | Pistol | 100% | 6.8 | 9.2 |  |
| marksman | Carbine | 100% | 7.7 | 9.9 |  |
| marksman | Rifle | 100% | 4.9 | 0.0 | fastest, safest |
| marksman | Blade | 100% | 9.2 | 5.7 |  |
| marksman | Greatblade | 100% | 12.4 | 5.7 |  |
| marksman | Glaive | 100% | 8.6 | 5.7 |  |
| duelist | Pistol | 100% | 1.7 | 0.0 | safest |
| duelist | Carbine | 100% | 1.3 | 0.0 | fastest, safest |
| duelist | Rifle | 100% | 4.6 | 0.0 | safest |
| duelist | Blade | 100% | 4.1 | 62.0 |  |
| duelist | Greatblade | 100% | 5.8 | 43.1 |  |
| duelist | Glaive | 100% | 2.4 | 31.0 |  |
| bulwark | Pistol | 100% | 10.7 | 24.3 |  |
| bulwark | Carbine | 100% | 6.3 | 11.7 |  |
| bulwark | Rifle | 100% | 8.3 | 11.7 |  |
| bulwark | Blade | 100% | 4.3 | 22.1 |  |
| bulwark | Greatblade | 100% | 4.1 | 14.0 |  |
| bulwark | Glaive | 100% | 3.6 | 0.0 | fastest, safest |

Against the sentry the quickest winner is the Pistol (2.2 s, taking 2.4). No melee class is within 1.5 times the best gun's (Pistol, 2.2 s, taking 2.4) in both seconds and damage taken; the nearest in seconds is the Glaive at 4.2 s, 1.9 times as long, taking 12.0.

Against the rusher the quickest winner is the Carbine (1.6 s, taking 0.0). No melee class is within 1.5 times the best gun's (Carbine, 1.6 s, taking 0.0) in both seconds and damage taken; the nearest in seconds is the Glaive at 2.6 s, 1.6 times as long, taking 27.5.

Against the marksman the quickest winner is the Rifle (4.9 s, taking 0.0). No melee class is within 1.5 times the best gun's (Rifle, 4.9 s, taking 0.0) in both seconds and damage taken; the nearest in seconds is the Glaive at 8.6 s, 1.7 times as long, taking 5.7.

Against the duelist the quickest winner is the Carbine (1.3 s, taking 0.0). No melee class is within 1.5 times the best gun's (Carbine, 1.3 s, taking 0.0) in both seconds and damage taken; the nearest in seconds is the Glaive at 2.4 s, 1.8 times as long, taking 31.0.

Against the bulwark the quickest winner is the Glaive (3.6 s, taking 0.0). Within 1.5 times the best gun's (Carbine, 6.3 s, taking 11.7) in both seconds and damage taken: Greatblade, Glaive.


## What changed in this tuning

Written from `assets/balance/combat_tuning.ron`, the log kept while the dials were moved.

| Dial | From | To | Why |
|---|---|---|---|
| progression/opponents.ron rusher.learned | blade.novice, blade.cuts.1 to 3 | blade.novice, blade.cuts.1 and 2 | Since step 7 One hand 3 gives the Heavy 1.15 of its posture, so the game's rusher swung 92 where the harness's rusher swung 80 and where it swung before step 7. Without the box the game's rusher reads what it read before: Heavy posture 80, damage 55, no other figure of the rusher moved. |
| opponents.ron marksman.recover | 2.8 | 2.2 | A carbine that stands and fires from 60 m won 0.33 of its bouts against the marksman; the band wants at most 0.15 (0.00 now). The wait after a shot is the one dial that moves it without making the marksman a different weapon. |
| opponents.ron marksman.cooldown | 1.5 | 0.9 | The wait before the first charge. The rifle that walks across the line and stops to aim needs about 5 s to win, and the marksman's first shot came at 3.1 s; at 2.5 s it fires in every bout, so the duel is a duel. |
| opponents.ron marksman.walk_speed | 2.2 | 1.8 | A greatblade cut roots its user for 0.70 s and a marksman that backs away at 2.2 m/s is never there when it lands (10 cuts, 0 landed, no bout won). At 1.9 or less the class catches it; 1.8 gives a margin. |
| opponents.ron duelist.rest | 0.3 | 0.5 | Step 8 had moved it to 0.3 to bring one single-trajectory fight (the guarding blade, 30.9 s) under the 30 s cap. On seeded cutting hands (60 bouts each way) the guarding blade wins 0.88 of its bouts inside 45 s at 0.5 (0.68 inside 30 s, mean 30.1 s) and 1.00 at 0.3 (0.93 inside 30 s, mean 24.1 s), so the rule to move it (under 0.6 inside 45 s at 0.5 and at 0.3) is not met and 0.5 stays; the band that reads the guarding blade is read inside 45 s. |
| tools/balance lib.rs rifle hand (walk, stand) | 1.8 s, 0.6 s | 0.9 s, 0.9 s | A policy figure, not a table: the rifle hand walks across the line and stands to aim. It was set by hand to give the pistol its middle, one shot every 3.3 s against the row's 1.8 s interval. It is now set by a rule: both are about the settling time of the aim (0.875 s), so a cycle is the interval of the row. The pistol holds 0.87 at 30 m and the rifle wins 1.00 at 80 m with it. |
| weapons.ron Rifle.aim_time | 0.7 | 0.875 | At 30 m the pistol won 0.17 to 0.45 of its bouts against a rifle that stops to aim; the band wants at least half. With the rifle hand set by the rule above, 0.7 reads 0.52 and 0.875 reads 0.87 over 60 bouts. |
| abilities.ron disarm.seconds | 2.5 | 1.5 | Against the duelist, as one trajectory at a rest of 0.3, disarm halved the damage a blade took (0.45 of the control) and at 1.5 s it read 0.90; the band wanted it between 0.625 and 0.97. Measured as a distribution at the shipped rest of 0.5 it reads 1.12 over four 60 bout windows (1.07 to 1.21), so the dial is not what makes disarm pay against this opponent and the band is unchosen. |
| abilities.ron charged_shot.damage | 1.4 | 1.1 | Charged shot killed the post at 60 m in 1.8 s against 3.1 s, 1.77 times the plain rifle; the band wants at most 1.60. At 1.1 it reads 1.24. |
| abilities.ron explosive_shot.damage | 1.0 | 1.6 | Explosive shot read 1.00 of the plain carbine against two targets side by side, dead. Its bolt, its blast, its windup and its length together make it 1.06. |
| abilities.ron explosive_shot.blast.damage | 12 | 24 | See explosive_shot.damage. |
| abilities.ron explosive_shot.windup, length, cancel | 0.40, 0.80, 0.60 | 0.20, 0.50, 0.30 | See explosive_shot.damage: the time the carbine does not fire was most of the price. |
| opponents.ron bulwark.guard.bolts.share | 0.90 | 0.97 | At 0.90 a held trigger killed the bulwark through the raised guard (44 blocked rounds against a pool of 105) in 4.7 s, quicker than the carbine that waits for the opening (5.4 s), so playing the gun earned nothing. At 0.97 holding fire takes 6.7 s and 17 to 19 damage, 1.2 times the waiting carbine's seconds. At 1.0 it reads 7.0 s; the stamina of 0.15 a point was left, since 0.5 breaks the guard in 3.7 s. |
| opponents.ron bulwark.pinned | (new) | 0.9 | Seconds a cut met by the guard holds the bulwark's own cut back. At 0.6 the greatblade's slower lights did not hold it (its kit took 45 against 14 at 0.9); at 0.9 every class holds it for as long as its stamina keeps the cuts coming. |
| opponents.ron bulwark.guard.stamina | 1.40 | 2.00 | At 1.40 the fifth light broke the guard and the plain blade took 5.6 s and 14; at 2.00 the third does (33.6 of the bulwark's 100 a blocked light) and it takes 4.3 s and 5. A bolt still spends 0.15 a point. |
| opponents.ron bulwark.stagger_cooldown | 0.8 | 1.2 | Now also the seconds a broken guard stays down past the stagger. At 0.8 the plain blade took 13 (the guard was back before the second cut of the opening), at 1.2 it takes 4 to 6, at 1.6 nothing, which is no fight. |
| opponents.ron bulwark.initial_cooldown | 0.4 | 0.9 | Now counted from the bulwark's arrival in reach, not from the start of the fight. At 0.6 the blade's heavy cut took 22; at 0.9 it takes 11, and the punishing carbine pays 0.3 s (6.0 s to 6.3 s). |
| abilities.ron focus windup, length, cancel | 0.15, 0.30, 0.30 | 0, 0, 0 | Made instant, the shape reflect shot has: the press costs a standing rifle nothing (the standing figure reads 1.00, was 0.91) and a rifle that crosses the line fires on the move while the status lasts. Seconds (5) and cooldown (18) are unchanged: on two posts at 60 m the moving hand reads 1.13 of itself. |
| abilities.ron butt_strike reach, posture, windup, length, cancel | 1.7, 70, 0.18, 0.55, 0.40 | 2.2, 105, 0.12, 0.40, 0.30 | It was never pressed on the bulwark (which holds 2.0 m off) and broke nothing when it was. It now reaches, lands before the cut it answers and breaks a full posture of 100, so the punishing carbine takes 0.0 where it took 11.7, for 0.4 s more (6.70 s against 6.27 s). Under 100 posture it breaks nothing and reads worse than no press. |
| abilities.ron fortified damage_taken | (new) | 0.5 | The status halves the damage taken, after any guard, beside holding posture; nothing else bears the field. |
| abilities.ron kinetic_absorb seconds, cooldown | 4.0, 20.0 | 2.0, 12.0 | Read on damage taken by the greatblade kit against the duelist. At 2.5 s it read 0.60 once a kit kept the stamina of its next cut, under the 0.625 floor; at 2.0 s it reads 0.68 (42.6 against 62.6) and the fight is 5.1 s against 7.4 s. |
| abilities.ron armour_break damage, posture, windup, length, cancel | 0.8, 1.0, 0.30, 0.70, 0.55 | 0.4, 0.5, 0.30, 0.70, 0.55 | Review of step 9c: the windup, length and cancel go back to their start, since 0.04 s of them moved the duelist fight more than the blow did. The greatblade pair (0.76, 0.86, 0.72 at three seeds earlier) certifies a pause: a press with the blow and status zeroed reads 0.79, so that band is unchosen. The rifle pair is the fight where the blow is the cause (3.93 s against 4.95 s on the marksman, 6.65 s with the blow zeroed). The melee Hit is a directed cut and a guard meets it; its status lands only when the guard is open. |
| abilities.ron disarm seconds, cooldown, windup, length, cancel | 1.5, 14.0, 0.20, 0.55, 0.45 | 3.0, 7.0, 0.10, 0.35, 0.25 | Why the blade that disarms took more: the press comes as the duelist's cut begins, and a cut under way is not stopped, so the 0.55 s commitment cost a blow and one disarm of 1.5 s spared none. A quicker hit with twice the seconds spares one: damage taken reads 0.83, 0.85 and 0.85 of the plain blade at three seeds. |
| abilities.ron explosive_shot blast damage, blast posture, cooldown | 24, 60, 16.0 | 48, 90, 10.0 | Read on seconds to win against the bulwark beside the punishing carbine: 0.90 (6.27 s to 5.66 s). A blast posture of 100 or more breaks the guard outright and ends the fight in 1.4 s, so 90 keeps it a blast and not a key. |
| abilities.ron reflect_shot seconds, cooldown, stamina; reflecting reflects | 4.0, 18.0, 30, 1.0 | 8.0, 9.0, 15, 2.0 | Read on a guarding blade against the sentry from 12 m: 1.28 of itself on damage a second. The kit blade closing from 27 m or 60 m gains nothing from it (1.00): its only cover is the thrust's 0.28 s, and it meets no bolt with the guard down. |
| abilities.ron sweeping_cut damage, cooldown | 1.1, 10.0 | 1.8, 6.0 | Against two rushers 1.39, 1.36 and 1.42 of the plain glaive on damage a second; against one 1.00. At a cooldown of 10 the fight (9 s) saw one sweep and the ability read 0.98. |
| abilities.ron vault distance, landing damage, cover share | 10.0, 1.0, 0.8 | 6.5, 1.2, 0.4 | Against the sentry from 27 m the kit with a 10 m leap took 2.4 where the kit without it took 25; at 6.5 m with a heavier landing and a thinner cover it takes 18 (0.70 of the control). Against the marksman from 60 m it still wins 1.00 where the control wins 0.3 to 0.5. |
| abilities.ron rapid_reload cooldown, windup, length, cancel, stamina | 20.0, 0.25, 0.40, 0.40, 30 | 10.0, 0.125, 0.2, 0.2, 15 | Read on the carbine against two dummies at 30 m, where the magazine runs dry: 1.08, 1.08 and 1.06 (was 1.035 to 1.04). |

28 dials moved.

