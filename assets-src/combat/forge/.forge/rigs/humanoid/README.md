# The humanoid rig profile

What a skinned mesh must be for the whole clip library to play on it,
unchanged. Every body the library holds — lifted from a picture through the
auto-rig, or arriving as a rigged file from anywhere else — is held to this
profile by `forge rig check` and by `forge promote body`, and the library
grows without the mesh knowing: a mesh that honours the profile rides along
for free. A mesh that is "close" does not degrade gracefully; it fails
silently (see the one rule below).

The profile is a directory of data, not a table in source:

| File | What it is | Who reads it |
|---|---|---|
| `contract.json` | Every bone: name, parent, whether clips drive it, rest transform. **Generated** from `rig.glb` by `forge rig export-contract` — never edited by hand. | `forge_rig`, rig check, promote, a game consuming the manifest |
| `sockets.json` | Where a prop rides: five named offsets from contract bones, plus the frame a prop is authored in. | `forge_rig`, a game's attach call |
| `motion_skeleton.json` | The driven layout (`cskel27`): the 27 joints a motion take writes, in take order, with the contact-column order the review reads. | `forge_motion` (asserted equal to its own constants), Python review |
| `profile.toml` | Every scalar the pipeline gates on — stature band, fit gate, tri budgets, matte look, finger layout, review thresholds — each with what it gates. | `forge_gen.profile` (Python), `forge doctor` |
| `rig.glb` | The skeleton as an artifact; the contract is derived from these bytes and carries their sha256. | `forge_rig::derive_from_glb`, the drift test |
| `rig.blend` | The same skeleton as a Blender file; the auto-rig opens it to skin every generated body, which is how a generated body is correct to the contract by construction. | `forge gen rig` |
| `fixture/cskel27_idle.glb` | A driven-only clip carrying the rest pose, so `forge gen rig-build` can rebuild `rig.blend` from scratch. | `forge gen rig-build` |

`forge verify` re-hashes `rig.glb` against the contract (a mismatch fails)
and `rig.blend` (a mismatch warns: the blend is the authoring source, not
what the contract was read from). A Rust test re-derives the bones from
`rig.glb` and holds `contract.json` to them within 1e-5; another re-runs the
exporter and holds the file byte for byte.

## The one rule: never add a parent above Hips

An engine binds an animation curve to a bone by hashing the bone's **full
name path** from the animation root — `Armature/Hips/Spine/...` — into an
id. Every clip in the library carries ids computed against this exact
hierarchy. Insert a `Root` bone above `Hips` and every path becomes
`Armature/Root/Hips/...`: every hash changes, every curve finds nothing, and
the engine reports **no error at any log level** — the character simply
holds its T-pose forever. The same applies to renaming any bone or inserting
one anywhere in the middle of the chain. Root motion does not need a root
bone here: it ships as the `Hips` translation curve and as per-clip metadata.

Adding **leaf** bones (a holster, a marker) is safe by the same logic — a
leaf adds a new path without rewriting any existing one — but the validator
still reports each one so a reviewer can see it; see [Extra
bones](#extra-bones).

This is why `contract.json` carries `parent` as an index and the validator
checks *depth*, not just names: a bone at the right name but the wrong depth
hashes differently and binds to nothing.

## The skeleton

55 bones. The 27 marked `*` are driven by every clip (`driven: true` in the
contract; the joints of `motion_skeleton.json`); the finger bones exist so
hands can be skinned, but **no clip ever animates them**.

```
Armature                      <- scene object, identity transform, NOT a bone
└─ Hips *
   ├─ Spine * ─ Spine1 * ─ Spine2 * ─ Spine3 *
   │   ├─ Neck * ─ Head *
   │   ├─ LeftShoulder * ─ LeftArm * ─ LeftForeArm * ─ LeftHand *
   │   │   ├─ LeftHandEnd *
   │   │   ├─ LeftHandThumb1 * ─ LeftHandThumb2 ─ LeftHandThumb3
   │   │   ├─ LeftHandIndex1 ─ LeftHandIndex2 ─ LeftHandIndex3
   │   │   ├─ LeftHandMiddle1 ─ LeftHandMiddle2 ─ LeftHandMiddle3
   │   │   ├─ LeftHandRing1 ─ LeftHandRing2 ─ LeftHandRing3
   │   │   └─ LeftHandPinky1 ─ LeftHandPinky2 ─ LeftHandPinky3
   │   └─ RightShoulder * ─ RightArm * ─ RightForeArm * ─ RightHand *
   │       └─ (mirror of the left hand)
   ├─ LeftUpLeg * ─ LeftLeg * ─ LeftFoot * ─ LeftToeBase *
   └─ RightUpLeg * ─ RightLeg * ─ RightFoot * ─ RightToeBase *
```

The contract lists bones in `rig.glb` node order, which is not this order
and is not alphabetical; nothing may reorder it, because `parent` indices
point into the list and drift is checked position for position.

## The numbers

Every one of these is in `contract.json` or `profile.toml`; this is the
reasoning behind them.

- **The rest pose is frozen.** Clips are baked against these exact rest
  rotations and the validator measures a mesh's skeleton against them
  (`export.rest_tolerance_m` = 0.1 mm in the exporter,
  `rest_rotation_tolerance` = 1e-3 per quaternion component in rig check).
  Nothing re-poses the rig to meet a mesh; the mesh is moved to meet the rig
  — which is why the generated path demands a strict T-pose *in the
  reference image* and the auto-rig's fit gate (`[fit]`: reach 0.80–1.45 of
  the wrist span, arm tips within 0.15 m of wrist height) refuses anything
  else. The message names the real fix: iterate the reference image, never
  the weights.
- **1.80 m reference stature.** Anything from 1.4 m to 2.2 m validates
  (`stature_m`); taller or shorter is refused — a prop mistaken for a body,
  or a mesh still in raw units.
- **Feet on the ground.** Lowest skinned vertex within 0.05 m of y = 0
  (`foot_tolerance_m`).
- **Facing.** The **rest** pose faces +Z in glTF axes (`front`): the toe
  bones sit at z = +0.16 in front of the ankles. Clips may carry a half turn
  so the animated character faces an engine's forward; anything measured
  against the rest pose — a socket offset — uses +Z as the front or comes out
  mirrored. Right is −X (`RightUpLeg` sits at x = −0.095).
- **Every vertex weighted**, ≤ 4 influences (`export.max_influences`),
  summing to 1.0. One-bone rigid weighting is acceptable; zero-weighted
  vertices are not. The auto-rig aborts when bone heat leaves more than a
  fifth of the mesh weightless (`rig.unweighted_abort_fraction`) rather than
  ship a statue.
- **The finger leaves will carry little or no weight** on a mitt-resolution
  mesh. That is the register, not a defect: the contract wants the bones
  present and every vertex weighted to *some* contract bone, not every bone
  deforming.
- **Self-contained file.** Materials exported, every image packed; a glb
  that references loose textures is refused at ingest
  (`forge_rig::measure`).
- **One body, dressed.** A character is a single skinned mesh wearing its
  clothes as texture; there are no garment parts and nothing is culled
  underneath anything. Equipment — a sword, a pistol — attaches to a socket
  and is a prop, not a part of the body.

## Sockets

`sockets.json` names five attachment points — `hand_r`, `hand_l`, `back`,
`hip_l`, `head` — each an offset from a contract bone, metres in the bone's
own space with a rotation that carries a prop's authoring frame into bone
space. A prop is authored **grip at the origin, long axis +Y, front −Z**
(`authoring_frame`); `forge gen prop --grip` turns a lifted weapon to that
frame, so a prop imported to the convention needs no per-prop correction and
the last centimetre of fit is the caller's own offset at attach time.

Each socket records its derivation in its `note`. Adding a socket is
additive — no bone is invented, nothing about the contract moves — so the
list grows without a `version` bump. The tests pin bands, not points: a
visual tune moves a number without rewriting a test, but a tune that moves a
socket to the far side of the body, or sends a blade into the wrong hand,
still fails loudly.

## Extra bones

A bone the contract does not name is allowed below a contract bone as long
as no contract bone hangs beneath it: a lone leaf is where a holster, a
muzzle point or a socket marker hangs. Nothing in the library animates them,
and a lifted body carries none. `forge rig check` reports each extra as one
note — `extra leaf bone Holster` — and a note is not a failure. A contract
bone *under* an extra is the inserted-bone failure from the one rule, and no
naming changes it.

## Export settings

`forge gen export` applies these headless (it is what promoting a body
runs), and refuses a `.blend` that breaks the contract before a byte is
written: glTF Binary; only `Armature` and the meshes; `+Y Up`; object
transforms applied so the armature object is identity
(`export.transform_tolerance`); no glTF leaf bones (they would appear as
`_end` strangers); ≤ 4 influences; modifiers applied; animation off;
materials exported with every image packed. The post-export self-check
proves the file is self-contained — one JSON chunk, one BIN chunk, no `uri`
anywhere — because a glb that references a texture on one disk renders pink
on every other.

## Regenerating the profile

```sh
forge gen rig-build                        # rig.blend (+ rig.glb) from fixture/cskel27_idle.glb + [fingers]
cargo run -p forge_rig --example export_contract -- rigs/humanoid   # contract.json from rig.glb
cargo test -p forge_rig                    # drift, layout, sockets, byte-for-byte
```

A rebuilt rig must match `contract.json` to `fingers.rebuild_tolerance_m`
(1e-4 m) and `fingers.rebuild_rotation_tolerance` (1e-3); a re-exported
contract must reproduce the committed file byte for byte. If either moves,
something about the rig changed: bump `version`, and expect every clip in
the library to need rebaking.

## Validating a body

```sh
forge rig check path/to/body.glb              # named findings, non-zero on FAIL
forge rig check path/to/body.glb --out sheet.png   # and render it playing the reference clip
```

Findings print one per line — `ok:`, `FAIL:`, `note:` — covering the bones
(all 55, exact names, exact depths, no stranger above or between them), the
rest pose, the skin weights, stature and foot placement, and finally that
the reference clip (`reference_clip`, the walk: it drives all 27 joints)
binds to the skeleton the way it binds to the rig. The contact sheet is the
fastest way to spot bad weights, candy-wrapper joints or feet through the
floor before a human does.

## A second profile

Another rig is another directory beside this one with the same seven files,
named in `forge.toml`'s `rig =`. Nothing in `forge_rig` knows the number 55
or the name `Hips`: the contract says which bone is the root, the layout says
which bones are driven, and the cross-checks (`RigProfile::load`) hold the
three files to each other — every socket on a contract bone, the driven
bones exactly the layout's joints, the driven parent chain collapsing to the
layout's parents.
