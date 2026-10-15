# 0008: Weapon pictures from the procedural geometry

Status: steps 1, 2 and 4 built 2026-10-10 ("implement using workflows");
the record is "As built" at the end. Step 3 waits for the user's look at
the sheets and the two stills in `scratch/review/weapons/`. Step 5 lands
with ticket 0007's screens. The user asked whether a 2D
outline of a weapon could be made "simple and automatic" from the
procedural geometry, then for an implementation plan. It serves
[ticket 0007](0007-crafting-into-the-game.md): the fitting screen's
drawing of the weapon (its step 2) and the icons of weapons and mods in
every slot (its step 1).

## What is wanted

A side-on picture of every weapon, cut into its chassis and its mounts,
made by the bake from the same recipes that will make the model, with no
hand drawing and no hand placement. The fitting screen stacks the pieces,
tints each by its state (standard, fitted, shut, in focus) and lets a
mount be picked on the picture. The same pieces are the inventory icons.

## What exists, checked

- `veyra_weapons::geometry::build(&Assembly)` returns an ashlar `Building`
  with one part for the chassis and one per mount, each an instance of
  its own. Modules are authored in the chassis's frame ("a shared datum
  preserves the grip frame"), so a piece needs no placement.
- The frame is X across, Y up, Z along the barrel. The workbench's Side
  view looks along X (`studio.rs`, yaw of a quarter turn, no pitch).
- A module's solid is a function of the weapon as well as the module
  (`module(weapon, slot, id)`), so a picture is per chassis, mount and
  module, not per module alone.
- `ashlar_manifold::mesh_building` gives each part's elements as triangle
  meshes with normals, one per material slot (`shell`, `metal`, `grip`,
  `accent`, `ceramic`, `lens`, `light`). The crate's own test meshes all
  twelve weapons and 41 modules in under two seconds.
- `tools/content` is the one binary that links the kernel, writes through
  `ashlar_content::Content` (atomic writes, a cache keyed by inputs) and
  already has the PNG encoder in its tree. The game loads PNG and mounts
  `generated://`.
- slotted's `image` node takes a path and a size and has no tint. Its
  `custom` node lets a game put a widget of its own in a screen.
- The Manifold binding has `project()`, which gives a solid's silhouette
  as polygons. This plan does not use it; see the decision below.

## Decision: a small shaded picture, not a vector outline

What I described to the user was a silhouette from the kernel's
projection. Having read the code, rasterising the triangles the mesher
already returns is both less work and a better picture:

| | Polygons from `project()` | Triangles rasterised side on |
| --- | --- | --- |
| Change to ashlar | A new call exposed on the mesher | None |
| What the UI can draw | Nothing as it stands: Bevy's UI has images, not polygons, so they would be rasterised anyway | The image, directly |
| Interior | None. A carbine is one flat shape | Each material in its own tone, shaded by its facing, so barrels read round and bevels read as edges |
| Flat silhouette if wanted | Yes | Yes: it is the image's alpha channel |

So each piece is a two-channel image: tone and coverage. Drawn with its
tone it is a small technical picture; drawn from its alpha alone it is the
flat outline. The screen chooses, and can change its mind without a
rebake. Vector outlines stay possible later through `project()` if a
stroked line drawing is ever wanted.

## Design

**The rasteriser** is `veyra_weapons::picture`, pure code over
`ashlar::MeshedBuilding` with no kernel and no Bevy, so the crate stays
in the Bevy-free row. Orthographic along X with the muzzle to the right,
a depth buffer per piece, sixteen samples a pixel. A sample's tone is its
material's fixed tone times a facing term (mostly how square the surface
is to the view, a little from above), written as luminance; alpha is
coverage. Every tone and weight is a named constant in one table at the
top of the module.

**The frame.** For a chassis, the envelope is the union of the bounds of
the chassis and of every module that fits it, padded. One scale per
chassis fits that envelope to 1024 pixels on its long side, which covers
the fitting drawing at the 4K interface scale. Each piece is cropped to
its own bounds and carries its rectangle in the chassis's frame, so the
client places pieces by data and a picked rectangle is a mount.

**The files**, under `generated/weapons/veyra/`:

```
pictures.ron                       the index, versioned
<chassis>/chassis.png              the body alone
<chassis>/<mount>.standard.png     the part it is built with
<chassis>/<mount>.<module>.png     each module that fits
<chassis>/whole.png                the stock weapon complete, for its icon
```

**The index type** is `strata_registry::pictures::PictureSet` (no Bevy):
per chassis its frame size and metres per pixel, per piece its mount,
module, rectangle and file. `tools/content` writes it and the client
reads it; both already depend on that crate. `veyra-weapons` returns
plain structs and knows nothing of the registry.

**The bake** is `content::weapons()` beside `crossings()` and
`roadside()`. For each chassis it builds a handful of assemblies (the
k-th build selects the k-th module of every mount at once, so the count
is the longest module list plus one, not their product), meshes each,
and takes each mount's part from the build that has it. Outputs go
through `Content`, cached on a hash of the recipes' meshes and the tone
table.

**For a person to judge**, the bake also writes one contact sheet per
maker to `scratch/review/weapons/`: every chassis assembled stock, then
each mount's alternatives in a row. This is how the pictures are accepted
before any screen exists.

**In the client**, the game reads `pictures.ron` at startup into a
resource, as it does the other baked layouts, and a missing file leaves
the resource empty. The drawing is a widget of the game's own,
`strata:weapon_diagram`: given a chassis, what is fitted, which mounts
are shut and which is in focus, it lays the pieces out by their
rectangles and tints each from four theme roles in the `strata.*` block.
Each mount's piece is focusable and answers a click with the same message
its row in the list does. With no pictures it draws labelled boxes, so a
headless run and a fresh clone still have a working screen.

**Icons.** An item's icon is its piece's file: `whole.png` for a weapon,
the module's own piece for a mod, from the first chassis that takes it.

None of this is simulation state. Nothing joins `world_version`, the
handshake or the server.

## Steps

| Step | Builds | Done when |
| --- | --- | --- |
| 1 | `veyra_weapons::picture`: the rasteriser and the envelope, with tests | For the patrol carbine: the muzzle's piece lies right of the stock's; the extended magazine's piece is taller than the standard feed's; every piece's rectangle lies inside the frame; two runs give the same bytes |
| 2 | `strata_registry::pictures`, `content::weapons()`, the files and the index, in `just content`; the contact sheets | `just content` writes a picture for every chassis and every legal mount and module pair and says how many; a second run takes them from the cache; the user has looked at the sheets |
| 3 | Tuning from that look: tones, the facing term, padding, and whether pieces want an inked edge where depth or material jumps | The user accepts the sheets |
| 4 | The client resource and the `strata:weapon_diagram` widget, with a `slotted-test` test | The test clicks the muzzle's piece and the muzzle mount takes focus; a run with no `generated/weapons` shows the boxes |
| 5 | The widget in the fitting screen and the files as icons | Lands inside ticket 0007, steps 1 and 2 |

Steps 1 to 3 depend on nothing in ticket 0007 and can be built now. Step
4 needs only slotted. Step 5 waits for the screens it belongs to.

## To confirm while building

- Confirmed 2026-10-10: a game registers a `custom` widget kind from
  Rust. `slotted::ui::screen::WidgetRegistry::register(kind, widget)`
  takes an `impl Widget`, whose `spawn(&self, ctx, params, children)` has
  the world. The diagram is such a widget.
- The envelope of the longest rifle against 1024 pixels. If small parts
  on long weapons come out under about 24 pixels, raise the long side.
- Whether `whole.png` reads at slot size (54 logical pixels) for the long
  weapons, or wants a thicker, simpler icon cut of its own.

## Not in this ticket

The weapon in the hand in the world and the turning model in the fitting
screen are ticket 0007's step 8. Finishes and maker colours are not baked
into these pictures: the pieces are neutral so the interface can tint
them by state.

## As built: steps 1 and 2 (2026-10-10)

One workflow: the rasteriser, the bake, checks, an independent review and
its fixes. Targeted tests, clippy, rustdoc and the format check pass on
`veyra-weapons`, `strata-registry` and `content`; the full `just ci` was
not run.

What the bake writes: 12 chassis, 113 pieces (12 bodies, 34 standard
parts, 67 modules) and 12 whole pictures, 125 PNG files of 352 kB and a
33 kB index. The patrol carbine's frame is 1024 by 398 at 0.87 mm a pixel.
The smallest piece is 35 by 27 pixels, so the 1024 long side holds.

Where it differs from the plan above:

- **A mount's piece is drawn with the chassis as an occluder**, not under
  a depth buffer of its own. The review found that pieces drawn alone did
  not stack into the weapon on ten of twelve chassis (a stock's tenon or a
  feed's top showed over the receiver). Now the stack matches `whole.png`
  everywhere but where two mounts overlap each other, at most 0.3% of a
  weapon. The cost: a mod's picture lacks what the body hides of it, which
  matters for its icon (step 5).
- **Two marks in the index.** `hidden`: the chassis hides the whole part,
  so its file is the part drawn alone and it is left out of the stack.
  `same_as`: the picture cannot be told from an earlier piece of the same
  mount. Three pieces carry one: the precision rifle's two feeds (hidden,
  authored on the far side) and the field blade's hooked edge (flat in the
  plane the view looks along, so both edges are an 11 pixel line).
- **No cache.** `ashlar_content`'s cache is private and holds only its own
  kinds. The pictures are rasterised on every run, in 1.9 s, and
  identical files are not rewritten. Accepted unless it grows.
- **The sheets are opt-in**: `content --weapon-sheets <dir>`, which
  `just review` passes. `just content` alone writes none.
- The index also carries the frame's origin in metres, and the whole
  picture sits beside `pieces`, not in it, so `pieces` is exactly the
  stack.

For the user at the sheets (step 3):

- The precision rifle's feed is on the side the picture does not see. The
  recipe could mirror it, or the picture could stay as it is with the mark.
- The field blade's edge is invisible side on. A blade may want its own
  view, from above.
- Tone: the body is one light grey. Whether that is the right neutral for
  tinting is the first thing to judge.
- Hairline seams: where the body's outline cuts a mount, the shared edge
  pixel is 75% to 97% covered in the stack. One pixel wide, at most 292
  pixels on a weapon.

## As built: step 4 (2026-10-10)

A second workflow: the widget, checks, an independent review and its
fixes. The client's UI tests (now with the diagram's), the game's tests,
clippy, rustdoc and the format check pass; the full `just ci` was not run.

- `strata_client::ui::weapon_diagram`: the `WeaponPictures` resource,
  which the game fills from `pictures.ron` at startup in its windowed
  branch only; the widget `strata:weapon_diagram`, whose params name the
  chassis and its mounts with their label, module and state; and the
  message `MountPicked { chassis, mount }`, written once for a click or
  the accept action on a mount.
- A pictured diagram is a well as wide as its parent holding a stage of
  the frame's exact shape, centred, with every piece placed in percent of
  it. The well is capped at 60% of the window's height. The review found
  the first version let the two pistols, whose frames are tall, push the
  buttons under them off the screen.
- A mount is a button in slotted's sense (focus, navigation, test id
  `weapon.mount:<key>`), sized to its piece's rectangle. A mount with no
  visible piece gets a small labelled marker. With no pictures for the
  chassis, a labelled box per mount.
- Tint is by theme role (`strata.weapon.piece.*`), set at spawn and
  multiplied into the picture, so each material's tone survives.
- `strata:weapon_preview`, behind `--open-weapon-diagram [chassis]`, is a
  stand-in to see the widget: pick a mount, walk its parts. It goes when
  ticket 0007's fitting screen lands.

Seen in a window, on the software renderer at 1280 by 720: the patrol
carbine and the service pistol, stock, sight in focus
(`scratch/review/weapons/diagram.png`, `diagram-service-pistol.png`).
Not yet seen in a window: the fitted, shut and hover tints, the markers,
the boxes of a run with no baked pictures, and the default 2560 by 1440
window, where the long weapons are magnified about one and a half times.

Left for step 5:

- Picking is by rectangle, not by the picture. On the combat pistol the
  standard feed's centre lies under the grip's rectangle, which is drawn
  later, so a click there picks the grip. Reorder that chassis in the
  bake, or test the piece's alpha in the pick.
- A small part in focus is hard to see: the carbine's standard sight is
  two posts. The fitting screen wants a focus cue that does not depend on
  the piece's area, and perhaps a least hit size.
- Whether the well should narrow to a short weapon. The pistol sits small
  in a wide dark box.

## As built: step 5 and the diagram's leftovers (2026-10-11)

Done with ticket 0007's step 2, which is the first screen to use them.

- **Icons.** `content::weapon_icon` fits every picture, the hidden parts'
  and the whole weapons' too, into a square of 128 pixels: its long side
  112, never more than doubled, centred, its tone lifted so the lightest
  pixel is white. Written beside the picture as `icon.<stem>.png` in the
  same two channels and named in the index (`icon` on a piece and on the
  whole; `PICTURES_VERSION` 2, an older index refused by name). The contact
  sheets gained a strip of them per chassis. The client hands slotted the
  icon as the item's image where the index has one
  (`ItemCatalogue::frozen_with`) and tints it by the item's colour.
- **The focus cue.** The mount in focus is bracketed: four corners round
  its rectangle, never closer than 44 units, with its name on a tag. Not a
  target, so a click inside it lands on the part. With no pictures the box
  in focus has a mark before its name.
- **The well narrows.** A weapon held to its height limit gets a well of
  its own shape in the middle of its parent, and a screen may set the
  limit (`DiagramParams::max_height`); the fitting screen sets 30 percent
  of the window's height.
- **The preview is gone**: `strata:weapon_preview`, its state, strings,
  roles and tests, and `--open-weapon-diagram`. The widget's own tests use
  a screen that is the widget alone.

Still open from step 4: picking is by rectangle, so on a chassis whose
parts overlap the later in the index takes the click.
