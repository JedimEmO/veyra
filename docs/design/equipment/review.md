# Equipment proposal review

Updated 2026-10-05. The [Rust assembly studio](../../../tools/weapon-workbench/README.md)
now includes the Vigil military line, additional components and finer procedural
surfaces. Gameplay integration, animation clearances and combat balance remain
future work.

## Delivered authoring content

- Four maker identities: Kestrel, Morrow, Bracken and Vigil.
- Twelve chassis: the original eight plus combat pistol, patrol carbine,
  marksman rifle and breacher.
- Forty-one compatible components and 24 named configurations. Every exposed
  slot has a replacement; old assembly variant names and module IDs remain valid.
- Dark military coatings, lean receivers, narrow stocks and recessed fittings,
  alongside refinements to the original equipment.
- Three self-contained material graphs for coating, machined metal and polymer,
  with restrained grain and surface wear.
- Maker filters, independently scrolling panels, presets, finishes, side/top
  inspection and stock reference comparison that preserves the fitted selection.
- Reloadable assembly RON, slot-named OBJ geometry, MTL, recipe/material/graph
  sources and 24 texture maps across four textured material slots.
- [Current authoring RON](data/workbench-catalogue.ron), generated from the same
  Rust catalogue as the studio without starting a renderer.

The original [JSON catalogue](data/catalogue.json), [loadouts](data/loadouts.json)
and [configuration report](data-review.md) remain the historical eight-chassis
paper package. That proposal also contains three implants, three consumables,
13 example loadouts and ownership/fitting rules. Its data is separate from the
expanded authoring catalogue and is not loaded by the game.

## Current verification

| Check | Result and limit |
| --- | --- |
| Targeted Rust tests | 15 pass: 11 domain/geometry/preset tests and four tool UI/material tests |
| Exhaustive authoring configurations | All 802 legal module combinations validate, round-trip across three finishes and stay inside proposed factor bounds |
| Persistence and compatibility | Original eight saved variants still decode; incompatible selections preserve the previous valid assembly |
| Procedural geometry | Tests mesh stock chassis and compatible replacements through Ashlar's manifold backend; this does not prove every combined animation clearance |
| Material graphs | All three resolve without an external graph library and round-trip through RON |
| Native UI exercise | Twelve real button actions select Vigil Recon, change finish, compare twice, switch to Side, explode, save, reset, reload and export; the fitted build is restored |
| Pointer interaction | Both panels scroll independently; All exposes all 12 chassis, fitting a grip updates geometry, Load restores the previous configuration, and mouse drag orbits the model |
| Stock reference | Native toggle and Save retain the fitted Recon modules in saved RON |
| Full gallery | All 52 full-size captures rendered and were visually reviewed: 24 stock/fitted, 24 presets, two exploded and two finish studies |
| Minimum window | Reviewed at 1120 by 800; footer controls wrap and camera framing adds horizontal margin so the fitted carbine clears both panels |
| Portable export | OBJ has 21 named objects and 10,462 triangles for Field Recon; indices and material references resolve, all 24 maps decode at 512 by 512, and the saved assembly retains its five fitted modules |
| Workspace `just ci` | Attempted; stops at formatting differences in concurrently edited crossing/bridge files, so workspace lint, tests and server-check are not established by this run |
| Separate `just server-check` | Dependency allowlists and headless/server builds pass; lower-crate tests stop in the unrelated flora sky suite with SIGTRAP. An isolated single-thread retry crashes with SIGSEGV, so the complete server check is not green |
| Combat and economy | No new absolute-profile, melee-parity, DPS, price or crafting-profit result is asserted |

The generated authoring RON also matches a fresh export byte for byte. Side and
top inspection captures were reviewed alongside the perspective gallery.

The original JSON package previously passed 26 rejection tests, ownership
consistency checks and 38 weapon configurations crossed with seven legal implant
subsets. Those 266 checks concern the original paper data, not the current
802 authoring configurations. The Python checker does not validate the Rust
catalogue, geometric intersections or runtime transactions.

Concurrent world-generation work is outside this change. Its formatting was
left untouched. Targeted Rust tests establish the workbench checks above, not
workspace-wide CI success.

## Design findings retained

**Keep a weapon viable before support items.** Stock paper examples carry no
supplies or implants. Future scenarios still need solo pressure, mixed encounters,
the precision rifle and an unbraced projector. A second weapon should be a useful
investment rather than mandatory insurance.

**Separate shotgun damage from repeated stagger.** One discharge needs a divided
pellet budget. A full damage/posture event per pellet would erase cutter and baton
roles. Baton pressure also needs an escape test after each posture break.

**Preserve resources when rebuilding a character.** Equipment fitting should
preserve current absolute pools, ammunition, heat and supply cooldowns. These
remain gameplay implementation requirements.

**Protect the modular art contract.** The frame is right-handed, +Z forward and
Y up, with +X toward the user's left. Grip axes and attachment face normals serve
different purposes. Front modules need obstruction envelopes shared with the
simulation. Combined assemblies still require visual and animation review.

**Keep expansion controlled.** The requested military line now has distinct
chassis and bounded replacements. Launchers, dual wielding, ballistic penetration,
durability economies and wholesale weapon construction remain separate work.

## Review on return

1. Compare Vigil's proportions and dark finishes against the original makers.
   Check whether the detail is sufficient without returning to bulky silhouettes.
2. Compare stock and named configurations. Check that replacement parts remain
   legible and preserve a recognizable chassis.
3. Review full combinations and intended hand/reload clearances before animation
   or gameplay integration.
4. Resolve the original gameplay questions: conventional projectile roles,
   melee breadth, sustained direct-fire plasma and support-item friction.

Absolute weapon tuning should follow actual mechanics and animation measurements
through the existing balance harness. The authoring catalogue and portable
exports do not yet equip these weapons in the running game.

## Reproduce checks

```sh
cargo test -p veyra-weapons -p weapon-workbench
cargo clippy -p weapon-workbench -p veyra-weapons --all-targets -- -D warnings
cargo run -p weapon-workbench -- \
  --catalogue docs/design/equipment/data/workbench-catalogue.ron
cargo run -p weapon-workbench -- --gallery scratch/weapon-workbench/gallery
cargo run -p weapon-workbench -- --output scratch/weapon-workbench/exercise \
  --capture scratch/weapon-workbench/exercise/result.png --exercise
PYTHONDONTWRITEBYTECODE=1 python3 docs/design/equipment/validate.py \
  --report docs/design/equipment/data-review.md
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s docs/design/equipment -p 'test_validation.py' -v
just ci
```

Regenerate the RON catalogue from Rust and the original configuration report
from JSON rather than editing generated records by hand. Capture and exercise
commands write into the selected output directory.
