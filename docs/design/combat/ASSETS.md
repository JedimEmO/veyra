# Combat asset provenance

The isolated Forge project is `assets-src/combat/forge`. Its `out/` directory
contains disposable intermediates and is ignored. The committed source set
contains reference imports, original raw takes, upstream receipts, the fitted
Blender body, rig/fit/map records, baked library assets, and the rig contract.
The game consumes `assets/characters/combat/frontier_ranger.glb` and
`melee.ron`; it does not require Python, Blender or the generation backends.

## Body

The ranger reference was created for this task with OpenAI Imagegen using the
existing included allowance. Forge imported it with its source attribution.
TRELLIS.2 lifted the character locally on the existing RTX 4090; SkinTokens
fitted and weighted its skeleton; Blender exported it through Forge's gates.
No purchase or new paid service was authorized or made.

### Body closure review, 2026-10-01: replacement shipped

The first reference produced incomplete rear helmets. The enclosed-helmet v2
reference fixed that silhouette, but its initially accepted seed-42 body had
open seat and shoulder/collar surfaces that the seven-view review missed.
Those holes were present in the raw lift, before preparation, skinning or
animation. Five new lifts reused `assets-src/refs/characters/frontier_ranger_v2.png`
inside the Forge project and were inspected outside this sandbox with nine-view
sheets and `assets-src/review_body.py` rear/shoulder crops:

| Seed | Grid / requested vertex budget | Closure decision |
| --- | --- | --- |
| 7 | 1024 cascade / 25,000 | Rejected: openings at thigh plates, back plate and belt. |
| 11 | 1024 cascade / 25,000 | Rejected: seat closed, but torn pauldron edges. |
| 99 | 1024 cascade / 25,000 | Rejected: openings at belt and back. |
| 42 | 1024 cascade / 100,000 | Rejected: seat and collar still open; [comparison strip](body-v3-rejected-closeups.png). |
| 42 | 512 / 25,000 | Accepted: seat, collar, shoulders, belt, hands and helmet with visor enclosed; [nine views](body-v3-views.png), [close-ups](body-v3-closeups.png). |

Raising seed 42's vertex budget to 100,000 did not close its seat or collar,
so decimation was not the cause. The 1024 cascade tore the thin dark undersuit
beneath the plates; the 512 grid preserved it and won the closure review.
The five lift receipts remain under `assets-src/refs/characters/`. The retained
sheets are 1024 px wide copies of the existing renders, not new renders. Their
close-up sidecars retain the source and inspection-script hashes.

The accepted lift has 27,108 vertices and 24,936 triangles. Preparation reports
11,724 mesh vertices and 24,509 triangles; the shipped glTF has **25,978
skinned vertices, 24,509 triangles, 55 bones and 1.80 m height**. These are
stage-specific counts; exported glTF vertices include attribute splits. The
SkinTokens fit and hip-symmetry gate passed on the first sample, with no fit
problems, no unweighted vertices and at most four influences per vertex.
The fit record retains three off-axis landmark warnings (right hip and both
toes). All pre-export checks passed in the external run; no gate was relaxed.

CPU promotion shipped the 512 body as `frontier_ranger` in library **0.1.1**,
with its Blender source, lift, rig and export provenance and `agent:claude`
attribution. `forge verify` passes 23 checks; `forge audit` reproduces all nine
clips, root tracks and footsteps, verifies their poses, and passes body
conformance; `forge manifest --check` matches. `forge bones` binds walk, run,
aim, fire, light, followup, heavy, dodge and death to this body with 27 driven
bones and zero orphaned curves each. The runtime bundle retains motion scale
1.0 and the required eight-clip order; `melee.ron` and the measured curves were
regenerated against it. All three attack peaks remain inside their unchanged
contact windows, as recorded in [REPORT](REPORT.md#melee-pacing-revision).
These CPU checks establish integrity and binding. Animated deformation,
weapon placement and rear/above-behind in-game screenshots of the new body
still need the external display session; no game or rendering was run here.

## Motion and equipment

Fresh ARDY text generation was unavailable because its configured text
encoder directory is missing. This work used existing ARDY `.npz` takes from
the user's Asset Forge checkout. Original upstream sidecars are retained in
`assets-src/imported/`. New clips were promoted without inventing unavailable
generation records: Forge correctly labels the new receipts `reconstructed`.

`tools/combat_bake.py` states every trim, in-place mode, Y policy, loop blend,
retiming knot, deformation setting and authored event. It reproduces all nine
library clips, including the reference walk. The runtime bundle order is run,
aim, fire, light, follow-up, heavy, dodge, death. Follow-up and heavy share the
same overhead-chop take with different phase timings. The light attack uses
the right-handed `ruin_explorer_attack` take; the earlier rusher take was
rejected for insufficient reach of the equipped hand.

The carbine is the existing `scrapyard_rifle` Forge prop. Its upstream receipt
is beside its GLB. Its mount is derived from the exported aim-pose hand
transform. The powered blade is an authored 0.78 m emissive primitive on the
right hand. Collider endpoints are generated from that exact mount and length.
Reload and stagger have simple procedural torso overlays, not dedicated ARDY
clips. Strafing uses a limited lower-body turn and a counter-turn at the spine;
backpedaling reverses the locomotion cycle. These are prototype compromises.

## Integrity and license facts

Forge `verify`, `audit`, and `manifest --check` passed. Audit reproduced all
nine clips byte for byte, their root tracks and contact footsteps, and verified
body conformance. The body claims integrity, not deterministic regeneration.
The runtime bundle has its own generated receipt and content hash. The curve
report records hashes of the actual bundle and generated contact curves.

The body and carbine receipts identify the nvdiffrast texture backend's
non-commercial license. These assets are prototype material; this work does
not establish commercial shipping clearance. The original rig/model/backend
license facts remain in the Forge records. No gated model terms were accepted
on the user's behalf.

`bash tools/combat_forge.sh …` runs against this project using the sibling
Asset Forge checkout (`FORGE_HOME` can override it). The wrapper expects a
local Forge executable in `target/combat-forge/debug/forge`, the build used
for this study. Forge commands and Python analysis are authoring tools only.

## Blaster revision after playtest

No new paid generation was used. Forge rendered the existing rifle in seven
views (`rifle-orientation.png`). Its barrel is local +Y and the grip extends
toward local +Z. The consumer mount now rotates 180 degrees about local +Y
after alignment, putting the grip below the barrel. The exported asset and
ARDY animation data are unchanged. The blaster now uses the levelled Forge
carbine takes documented below.


## Sound library, 2026-10-01

Library **0.1.2**, bumped from 0.1.1, includes 58 levelled takes for 24 events.
The game ships identical copies in `assets/audio/combat/`. The five former
placeholder WAVs have been removed. No sound generation or audio playback was
performed in this sandbox.

The source prompts are `assets-src/combat/forge/assets-src/sfx/combat.batch`,
one line per take. Rendering happened outside the sandbox with `[make] sfx = true`,
`forge gen sfx --prompt ... --seed ... --steps 100`, created by `agent:claude`.
Takes within each event used distinct seeds, CFG 4, and requested durations
from 0.5 to 4 s; some seed values recur across different events.
The batch-mode run was abandoned because it reused one seed across takes.
`impact_flesh_3` and `swing_light_3` were silent backend rejects and were deleted.
Their batch entries remain as the record of the attempted set. Some retained
renders were extremely quiet, including `swing_light_1`; their measurements are
technical checks, not an audition of their content.

The backend was **MOSS-SoundEffect-v2.0**, with **Apache-2.0 model weights**,
executed inside **ComfyUI (GPL-3.0)** through the **TTS-Audio-Suite pack (MIT)**.
ComfyUI is a separate authoring service; nothing from it is linked into the game.
The generator records retain prompts, seeds, steps, CFG, duration, output hashes,
ComfyUI commit, workflow hash and pack commit. These are the supplied backend
licence facts, separate from the body/carbine licence limitations above.

`tools/combat_audio.py` replaces the old placeholder recipe. Run
`python3 tools/combat_audio.py` with ffmpeg 6.1 and Python's standard library.
It archives the unmodified WAVs and JSON receipts from `out/audio/sfx/` into
`assets-src/combat/forge/assets-src/sfx/takes/` and uses that archive when `out/`
is absent. It removes DC with a 30 Hz high-pass, trims leading silence relative
to each take's peak, retains 5 ms before the transient, applies measured EBU R128
gain with an oversampled peak limiter, removes any DC introduced by limiting,
and adds 5 ms fades at both ends. Empty tails are trimmed too. For files shorter
than 400 ms, a three-second repetition is measured for analysis only; the shipped
file retains its short duration. This avoids boosting empty padding or noise to
meet an integrated loudness target. Sentry anticipation is compressed to fit its
650 ms gameplay charge without changing pitch. Outputs are 48 kHz,
mono, 16-bit PCM WAV. `assets/audio/combat/levelling.json` records input/output
hashes, script hash, ffmpeg version, every filter, gain and measured result.

Only one `low_health` render was supplied and that single take is retained,
as requested. The table explicitly marks loop events: the asset test requires
at least one take for a loop and at least two for every one-shot event. The
derived second low-health variant was removed from the recipe, game assets
and Forge library. The loop fades to zero at both endpoints; subjective loop
rhythm and the very quiet source takes still need an external listening pass.

Each processed file was promoted using `forge promote audio sfx <file> <name>
--record <original-json> --created-by agent:claude`. Forge accepted the processed
files with the raw generator records. Promotion notes point to the retained raw
source and reproducible levelling step; the records themselves were not altered.
`forge verify` passes **139 checks**; `forge manifest --check` matches library
0.1.2. `forge audio list ../../../assets/audio/combat` measures all 58 files
without defects or warnings. Forge's LUFS column is an approximation (60 Hz
high-pass and 400 ms blocks without a relative gate), not EBU R128; it differs
from ffmpeg's K-weighted, gated integrated loudness. Both readings are below.

| Event | Files | EBU R128 LUFS range¹ | Forge approximate LUFS range |
| --- | ---: | ---: | ---: |
| `blade_hit_armor` | 3 | -14.2 to -14.0 | -18.2 to -17.3 |
| `blade_hit_shield` | 2 | -14.0 to -14.0 | -18.2 to -15.1 |
| `blaster_shot` | 3 | -14.2 to -14.0 | -18.7 to -16.8 |
| `dodge_roll` | 2 | -24.1 to -24.0 | -27.9 to -25.6 |
| `enemy_death` | 2 | -18.0 to -18.0 | -23.2 to -20.5 |
| `foot_gravel` | 4 | -28.1 to -27.9 | -33.5 to -30.9 |
| `foot_metal` | 4 | -28.1 to -27.9 | -31.4 to -25.1 |
| `foot_stone` | 4 | -28.1 to -28.0 | -31.3 to -25.2 |
| `hit_marker` | 2 | -20.2 to -20.0 | -24.0 to -23.5 |
| `impact_flesh` | 2 | -16.1 to -16.1 | -17.4 to -16.8 |
| `impact_metal` | 2 | -16.1 to -16.0 | -20.2 to -19.8 |
| `impact_shield` | 3 | -16.1 to -16.0 | -20.3 to -17.0 |
| `impact_stone` | 3 | -16.1 to -16.1 | -22.9 to -21.7 |
| `jump` | 2 | -24.1 to -24.1 | -22.7 to -20.1 |
| `land` | 2 | -24.1 to -24.1 | -27.7 to -22.1 |
| `low_health` | 1 | -22.0 to -22.0 | -21.1 to -21.1 |
| `player_hurt` | 3 | -18.1 to -18.0 | -22.7 to -18.5 |
| `reload` | 2 | -24.1 to -24.0 | -29.1 to -25.4 |
| `rusher_windup` | 2 | -18.2 to -18.2 | -20.0 to -18.3 |
| `sentry_charge` | 2 | -18.0 to -18.0 | -22.2 to -20.4 |
| `sentry_shot` | 2 | -14.2 to -14.1 | -15.5 to -14.4 |
| `shield_recharge` | 2 | -22.0 to -22.0 | -23.9 to -23.0 |
| `swing_heavy` | 2 | -14.2 to -14.1 | -16.6 to -16.4 |
| `swing_light` | 2 | -18.1 to -18.0 | -18.4 to -18.3 |

¹ Clips shorter than 400 ms use the repeated-transient measurement described
above. `levelling.json` identifies the measurement method for each file.

### Playback cues

The client table selects one-shot takes randomly without immediate repeats and
applies per-event mix gains, plus 2 to 4 percent speed/pitch variation. The
single low-health loop and timed sentry warning keep their authored speed.
The master-volume slider updates existing sinks as well as new sounds.
Enemy cues and all impacts use Bevy spatial playback with
the combat camera as listener. Emitters retain world positions across origin
rebases. Player shots, swings, dodge, reload, footsteps, jump, landing, hurt and
UI feedback stay non-spatial.

- `ShotFired`: local blaster or enemy sentry shot at the confirmed launch.
- `CombatImpact`: shield versus flesh for bolts, shield versus armor for blades;
  confirmed outgoing hits also tick the hit marker. The current impact message
  carries no scenery material, so scenery uses stone. Metal impact takes are
  registered for a future material-bearing cue; no metal surface is guessed.
- Accepted local actions: light/follow-up swing, heavy swing, dodge and reload.
  Enemy melee start plays the rusher windup; enemy death plays once on transition.
- Sentry charge starts with its 650 ms anticipation, follows the enemy, and stops
  at launch, interruption or death. Player hurt follows the confirmed hit counter.
- Footsteps come from the run clip's audited contacts at 0.15 and 0.55 s in its
  0.8 s loop, using the very same phase as the rendered animation. Reverse motion
  crosses the same contacts; airborne, stationary and committed action poses
  suppress steps. Terrain's exposed material selects stone, gravel or metal;
  without a recognized surface, use stone inside a city's site and gravel outside.
  Current terrain has no metal layer, so metal footsteps await such a surface.
- Jump/land follow the controller's actual grounded transitions; walking off an
  edge does not play jump. Low health loops below 25 of the player's 100 HP and
  stops on recovery, death or player removal. Shield recharge sounds once at the
  start of a recovery period, including recovery that resumes after new damage.

The headless encounter does not install the client audio plugin. Its smoke run
therefore checks simulation startup and logs; the client unit test separately
opens every table file, asserts 48 kHz mono 16-bit PCM and the minimum take
count (two for one-shots, one for loops), and decodes all samples through Bevy's
actual decoder without an audio device or display.

Validation completed with no GPU, display or audio output: `cargo fmt --all`
and `cargo fmt --all -- --check`,
`cargo clippy -p strata-client -p game --all-targets -- -D warnings`, and
`cargo test -p strata-client -p game` (25 game, 32 client unit, 5 client
integration tests) passed. Other agents' settlement edits were left alone.
`cargo run -p game -- --headless --combat-script
--frames 600` exited successfully after 600 frames with no warnings or errors,
including no missing-asset or audio-decode messages. Re-running the revised
levelling script reproduced all 58 files byte-for-byte against their existing
Forge promotions. Source, recipe, game-copy and library-copy hashes match;
levels are within 0.2 LU of target and the highest measured true peak is
-2.41 dBTP. No subjective listening or in-game spatial balance was assessed.
