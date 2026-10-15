# Play the combat slice

```sh
just run
```

The slice places a generated ranger, a passive target, a melee rusher, and a
ranged sentry beside the western edge of Port Serein. The camera starts facing
the city.
Local character play enables combat by default; `--combat` remains an alias,
and `--no-combat` selects the original controller. `--spawn x,z` overrides that
placement. The world and its terrain remain the ordinary streamed world.
Use `--window 1280x720` for a smaller window. Let the nearby terrain finish
streaming before judging visual smoothness.

| Input | Action |
| --- | --- |
| WASD / Shift | Move / sprint |
| Mouse / RMB | Look / aim |
| LMB / R | Fire / reload |
| 1 / 2 | Draw the carbine / the pistol (0.45 s; waits through a reload or a cut; each keeps its magazine) |
| E | Fast blade thrust; press again during contact to chain immediately |
| Q | Heavy overhead strike |
| Left Alt or Left Ctrl | Dodge in the movement direction; forward if stationary |
| Space | Jump |
| F5 | Reset actors, resources and positions |
| Escape / F3 | Menu / debug statistics |

Enemies wait until you approach within 11 m or damage a hostile. The passive
target stays passive. Orange light on the sentry announces its 650 ms charge;
its aim is fixed during that charge. Orange bolts travel at 24 m/s; your cyan
bolts travel at 38 m/s. Strafe out of the firing line, close during recovery,
and use the blade to break posture. Two light attacks and a heavy are a substantial stamina
commitment. Dodge becomes available in the late recovery of a melee attack;
it cannot erase its contact commitment. Shields recover after three seconds
without damage. Empty magazines require a completed reload.

[Design and equations](DESIGN.md), [measured findings and limits](REPORT.md),
[asset provenance](ASSETS.md), [blaster response curves](curves/blaster-response.png),
[light](curves/baked-light.png), [follow-up](curves/baked-followup.png),
[heavy](curves/baked-heavy.png).

## Repeat the study

```sh
just run --combat --combat-script --combat-trace docs/design/combat/curves/blaster-runtime.csv \
  --frames 3000 --window 1280x720 --serial-pipelines --time 0.5 \
  --shot-frames 550,1600,2700 \
  --screenshot 'docs/design/combat/blaster-final-{frame}.png'
```

The script warms up, demonstrates the moveset on the passive target, then
releases the other enemies at 22 simulation seconds. It does not grant
invulnerability. Screenshot readback can keep the app alive beyond the
requested frame count. Frame positions include world loading, so the CSV
action clock is the precise timing record.

The final script also fights the two hostiles from 22 seconds onward, using
ordinary firing, strafing and reload inputs. The HUD shows AREA CLEAR when
both are defeated. F5 resets the encounter and removes every in-flight bolt.

`--combat-trace path.csv` also records interactive sessions. For simulation
alone, add `--headless` and omit screenshot arguments. Combat currently uses
a standalone 60 Hz clock for local play. `--connect` automatically selects the
original networked
controller and protocol. Flight, route, auto-walk and terrain study modes also
retain their existing controllers. Explicit `--combat` refuses those modes.

Run the following with Python containing NumPy, SciPy and Matplotlib. The
installed ARDY environment contains these packages.

```sh
python tools/combat_curves.py --bundle assets/characters/combat/frontier_ranger.glb \
  --trace docs/design/combat/curves/blaster-runtime.csv
python tools/combat_projectiles.py
```

That regenerates the figures, measurements and headless `melee.ron` from the
same GLB the client plays. Keep the GLB, curves, and reports together when
changing an attack.

## Energy fire capture

Light now contacts at 130–220 ms and finishes in 400 ms. A press during
contact chains into the 120–220 ms follow-up on the tick contact ends; heavy
contacts at 320–450 ms and finishes in 850 ms. See
[Melee pacing revision](REPORT.md#melee-pacing-revision) for before/after
measurements and the validation limit.

Day and night captures are pending: the editing session could not open an
X display and exposes no GPU. The following paths are intended outputs,
not existing visual evidence:
`bolts-v2-550.png`, `bolts-v2-1600.png`, `bolts-v2-night-550.png`,
`bolts-v2-night-1600.png`.

```sh
xvfb-run -a -s "-screen 0 2560x1440x24" cargo run --release -p game -- \
  --combat-script --frames 1800 --window 1280x720 --serial-pipelines --time 0.5 \
  --shot-frames 550,1600 --screenshot 'docs/design/combat/bolts-v2-{frame}.png'
xvfb-run -a -s "-screen 0 2560x1440x24" cargo run --release -p game -- \
  --combat-script --frames 1800 --window 1280x720 --serial-pipelines --time 0.95 \
  --shot-frames 550,1600 --screenshot 'docs/design/combat/bolts-v2-night-{frame}.png'
```

Inspect the cyan and orange bolt cores, glow and trails at 20–40 m, and
capture additional adjacent frames if the three-frame muzzle flash or
impact falls between these samples. The scripted FPS line is nominal 60;
compare wall time and CPU/GPU timings too when measuring effects cost.
