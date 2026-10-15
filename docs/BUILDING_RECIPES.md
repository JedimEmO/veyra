# Buildings

The authoring system is [ashlar](../../../bevy-crates/ashlar), a standalone
workspace beside this one: `ashlar` is the Bevy-free recipe domain, its
`ashlar-manifold` adapter evaluates solids through the native Manifold kernel,
`ashlar-bevy` uploads the meshes and PBR materials, and `ashlar-preview` is the
viewer and the reference gallery. The authoring contract, the validation rules,
the UV projection, sockets, derived collision and the terrain solver are
documented once, in
[ashlar's recipe guide](../../../bevy-crates/ashlar/docs/guide/recipes.md) and
its two ADRs. This page is what belongs to the game.

## The Veyra content

`crates/veyra-buildings` is game content over that domain: the study kit
(`outpost`, `office`, `facade`, `entrance`) and the metropolitan kit
(`corporate`, `corporate-annex`, `corporate-block`) in
`veyra_buildings::corporate`, and their grounding contract. Every dimension and
layout is a decision of this crate; nothing in ashlar knows about them.

Their finishes are `crates/veyra-materials`: one definition per key the kits
bind (`study:*`, `metro:*`), each either a material graph with our parameters
or a constant. Nothing is a texture file. `just content` bakes the graphs into
KTX2 maps and the buildings into `.ashlar` files under `generated/`, which the
game mounts as the `generated://` asset source; the output is regenerated,
never committed.

Elements declare their own collision, so a proxy set is derived from the same
evaluated solid the mesh comes from and cannot drift from it. There are no
hand-written collider tables any more; `crates/veyra-buildings/tests/collision.rs`
keeps the last version of the old ones as the fixture that shows the derived
set covers what they covered.

```sh
# Bake the buildings and their materials into generated/.
just content
# The viewer, over the Veyra scenes. It bakes the graphs itself, so a material
# edit shows here without a content step.
just building-recipe-preview
just building-recipe-preview --scene corporate-block --serial-pipelines
just building-recipe-preview --screenshot /tmp/building.png
# Every scene from three fixed angles into target/building-references/.
just building-references
```

`tools/building-preview` is a table of scene names over
`ashlar_preview::Catalog` and nothing else.

## The grounding fixture

`just building-grounding <site>` fits a building to the real world at three
repeatable seed-42 sites (`flat`, `slope`, `uneven`) and walks the character up
to its threshold:

```sh
just building-grounding uneven -- --frames 90 --screenshot target/grounding.png
just building-grounding slope -- --grounding-style corporate --frames 90
```

`apps/game/src/grounding.rs` is the fixture. It reads the building's derived
proxies from level 0 of its baked file before the event loop, hands them and the
fitted supports to `strata_physics::grounding::collider`, which builds one
convex compound out of both, and spawns the building itself through
`AshlarPlugin` from the same file. The game links no geometry kernel; run
`just content` first. `strata_physics::grounding::sample_patch` gathers the 81x81, one-metre
patch the solver reads, matching LOD0 collision. Vegetation reservations are
installed before streaming, so foliage and tree collision both avoid the site.
The ordinary world loads no building fixture; `--spawn` and `--look` override
the arrival view. See [site measurements, captures and
verification](research/building-grounding/README.md) and the [corporate
mockups](research/building-corporate/README.md).

World positions stay `f64`, and the visual root and its separate physics body
both follow `RenderOrigin`. There is no live refitting or building streaming
yet: the fixture spawns every level with its band and never disables any. The
doorway is an opening cut inside the entrance panel and a derived
proxy is convex, so collision seals it: the walker climbs the ramp and stands
on the threshold against the closed face rather than stepping into the
doorway. Making a kit passable means authoring the opening as the gap between
elements, which is ashlar ADR 0002's stated price for deriving collision from
a boolean result.
