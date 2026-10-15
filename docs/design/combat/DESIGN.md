# Frontier combat slice

Direction agreed 2026-10-01: a playable slice and measured animation study,
balanced gunplay and melee, mouse/keyboard first, no purchased generation.

## Experience

Cross open ground under fire, use terrain for cover, shoot while moving, then
commit to a close-range opening. A blade should feel useful enough to draw
deliberately. Its advantage is stagger and a decisive finish at close range;
the carbine's advantage is reach and continuous pressure. No cover snapping
or forced target lock. The camera and movement remain useful while enemies
circle. A missed heavy strike leaves a readable recovery to punish.

The first equipment pair is a compact carbine and a powered cutting blade.
Light attacks form a two-strike sequence; heavy attacks break posture. A
directional dodge provides brief invulnerability, followed by vulnerable
travel and recovery. Stamina pays for melee and dodge. Shooting and ordinary
travel remain available without stamina. Encounters begin with a melee rusher
and a ranged sentry, with a passive target for isolated tests.

## Implementation boundary

Local character play now starts this encounter by default in the open world.
`--no-combat` opts out. Existing connected play retains its protocol. Combat networking is a
subsequent integration gate, not a claim made by this prototype.

Rules and action clocks belong to `strata-sim`; spatial queries and motor
integration belong to `strata-physics`; input, camera, animation and effects
belong to `strata-client`. Enemy placement and encounter setup belong to
`apps/game`. World positions remain f64. Animation never writes the gameplay
root, and hitstop never pauses the world or changes authoritative hit timing.

The study uses a 60 Hz fixed clock and a matching `SimClock` step. This is an
local simulation rate; the current network protocol still runs at 20 Hz. A production integration
must agree on the rate at handshake, separate snapshot rate from simulation
rate, replicate action sequence/start time and resources, and reconcile the
complete motor/action state. The existing position-only correction is not
sufficient for predicted stamina, reloads, invulnerability or hit confirmation.

## Feel contract

Every number below is an authored target, including the melee pacing revision. Asset measurements and
runtime evidence are recorded separately; a target is never a measurement.

| Property | Current target | Reason |
| --- | --- | --- |
| Jog / sprint / aimed speed | 5.5 / 8.5 / 3.6 m/s | Cover distance without making nearby threats trivial |
| Ground acceleration / braking | 38 / 52 m/s² | About 145 ms to full jog, about 106 ms to stop |
| Air acceleration | 10 m/s² | Some correction, with an arc that remains legible |
| Jump buffer / coyote time | 120 / 100 ms | Forgive small input and ledge timing errors |
| Action input buffer | 150 ms, one request | Preserve intention without storing an accidental combo |
| Light 1 | 130 ms anticipation, 90 ms contact, 180 ms recovery (400 ms total) | Quick opening thrust |
| Light 2 | 120 / 100 / 180 ms (400 ms total) | Buffered during light contact, starts as that contact ends |
| Heavy | 320 / 130 / 400 ms (850 ms total) | Deliberate windup and punishable miss |
| Dodge | 480 ms, 3.4 m unobstructed | A deliberate reposition with bounded range |
| Dodge invulnerability | 60 to 240 ms | Visible startup and vulnerable ending |
| Blaster | 240 ms shot interval, 38 m/s bolts, 24 rounds, 1.35 s reload | Visible pulses with time to lead a moving target |
| Sentry | 650 ms locked aim charge, 24 m/s bolts, 1.65 s recovery | Read and leave its firing line |
| Incoming damage | Sentry 12; rusher heavy 27.5 | Several mistakes remain recoverable |
| Hostile resilience | 85 health + 20 shield | Five bolts, two heavies, or a light chain plus heavy |
| Stamina | 100; light 18, heavy 32, dodge 26 | Two or three commitments before a reset |

Movement follows a bounded acceleration toward the desired velocity:
`v_next = v + clamp_length(v_target - v, a * dt)`. The ground projection must
preserve analog input magnitude. Reversal uses braking before acceleration;
collision removes blocked travel rather than accumulating a launch impulse.

Dodge displacement uses `x(u) = D * (1 - (1 - u)^3)`, `u = clamp(t/T, 0, 1)`.
The swept displacement of a tick is `x(t + dt) - x(t)`, so changing frame rate
cannot change its distance. The curve has a strong initial push and a long
decelerating tail. It is passed through the capsule sweep, never teleported.

Attacks have separate anticipation, contact and recovery intervals. Hit tests
clip their time interval to the contact interval and sweep the blade path
through it. One action sequence can damage a particular victim once. Melee
must not pass through walls. A buffered light request begins the follow-up in
the same fixed tick that light contact ends, after physics has resolved that
last sweep with the outgoing action's identity. Damage can still interrupt the
chain. Both lights cancel into dodge at 310 ms; heavy at 690 ms. Authored lunge
distances are 0.50 / 0.60 / 0.85 m for light / follow-up / heavy, using the
same collision-constrained integrated ease-out. Presentation hit-stop is 24 ms
for lights and 36 ms for heavy, with a 140 ms victim flinch. Bolts sweep a 0.07 m sphere along each tick
segment, stopping at the nearest obstruction. The launch direction points from
the muzzle toward the crosshair's world target and never follows that target. A ray from the camera alone would
allow shooting through the cover hiding the weapon.

Camera zoom and shoulder offset use `alpha = 1 - exp(-lambda * dt)`; mouse
orientation is direct, so smoothing does not add aim latency. A collision
sweep shortens the camera boom. Recoil and hit feedback are local presentation
impulses, with adjustable small magnitudes. No global time dilation.

## Blaster response model

For a bolt launched at `p0` in direction `d`, `p(t) = p0 + v*d*t`, until
collision or 120 m of travel. Position remains f64 across origin changes.
Gameplay sweeps the full segment even when a bolt crosses a thin obstacle in
one tick. A blocked shoulder-to-muzzle segment dissipates on that obstruction;
an actor touched at the muzzle receives the same shot damage.

Firing feedback is immediate: a launch message drives the sound, muzzle flash
and camera impulse. Damage, impact sound and the hit marker occur only at the
collision. At 20 m, ideal centre-to-centre travel takes 526 ms for the player
and 833 ms for the sentry. Actual muzzle-to-capsule travel is slightly shorter.
Nine shots fit a two-second trigger hold at 30, 60 and 120 simulation Hz.

Sentry targeting captures a world position before its 650 ms charge, then
holds that position through launch. With no prediction or homing, moving
sideways displaces the player from that line. Enemy speed is 3.1 m/s for the
rusher; aimed player movement remains 3.575 m/s. A rusher's 320 ms heavy windup
locks direction, and its 2.6 s attack interval leaves recovery for retaliation.
A four-second initial grace allows streaming before proximity can engage the
encounter. Passive practice does not trigger aggression by itself.


## Animation study

Use Asset Forge for the character and ARDY for motion. Keep original reference,
generation and rig records, raw takes, explicit bake recipes, and export
receipts. Inspect the raw body from front, back and both sides before skinning,
then inspect strips on the actual skinned body. A valid skeleton alone does
not establish good animation.

Measure world-space root displacement, planted-foot travel, pelvis height,
hand and blade-tip trajectories, tip speed and acceleration. Plot source time
and gameplay time with anticipation/contact/recovery and cancel windows. ARDY
samples at 20 fps, so derivatives describe those samples and interpolated
motion, not hidden high-frequency detail.

Retiming is a monotonic piecewise map `source_time = f(gameplay_time)`.
Then `v_game = v_source * f'` and
`a_game = a_source * (f')² + v_source * f''`. Excessive rate changes create
visible snaps even when poses look sound. Choose contact at a measured sweep,
compress anticipation cautiously, and retain enough recovery to communicate
mass. Foot sliding is measured during contact; animation speed follows actual
motor travel during locomotion. Cosmetic recoil may layer over a stable aim
pose; locomotion must not pull the reticle off its target.

The analysis must distinguish proposed envelopes, measured clips, and captured
runtime behavior. It must also report whether a human has played the slice.
Curves make tuning reproducible; they cannot certify that combat is fun.

## Acceptance

Run action/state tests, collision and occlusion scenarios, input/menu gating,
and the repository's `just ci`. Capture the player and enemy on real assets,
including movement, both light strikes, heavy, dodge, shot and reload. Check
30/60/120 Hz analytical integration, one-hit-per-target, stamina exhaustion,
death, reset, and obstruction. Record actual passes and blockers without
changing terrain files to make an unrelated failure disappear.
