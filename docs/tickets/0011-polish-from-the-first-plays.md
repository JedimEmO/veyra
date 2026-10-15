# 0011: Polish from the first plays

Status: planned 2026-10-14; steps 1 to 4 built 2026-10-14. From the user's comments after playing tickets
0007 to 0010. Singleplayer still: nothing here touches the protocol,
`world_version` or `apps/server`.

## What the user said (2026-10-14)

> I want to be able to change weapons everywhere, not just at a work bench. I
> want a map so I can see discovered deposits etc., the screens need a fixed
> size, the whole layout shifts with varying height tooltips now (especially
> weapons). I want some more sounds/animations for sampling speedbike etc.,
> it's unclear to me how I assign abilities to my tool bar and see what
> abilities I actually have. abilities generally also need descriptions. I
> want more juice/polish, possibly particle effects in combat. the craftables
> list/craftable UI is a bit messy, maybe show better categorization, and only
> show craftable blueprints (at least by default). also, we really need item
> icons proper not just placeholders

## What is there today (read 2026-10-14, nothing run)

- **Weapons.** Pack to hand already works anywhere: `EquipRequest::Take` has
  no place rule for the pack. The only route is pinning a weapon on the pack
  screen and pressing Take in hand in the right pane, and a click on a hand
  with a weapon on the cursor is refused with "Changing what is fitted is a
  workbench's business.", which is what reads as "only at a workbench". A
  swap is not refused mid action. Fitting a part does need a workbench and
  stays so.
- **Map.** None. `Surveyed.found` is the only record of discovery; sites,
  roads, rivers and lakes are in the baked network; `apps/worldmap` paints
  them but its painters live in the binary. The icon library has map symbols.
- **Screen sizes.** Crafting and the factory are one frame (`min_height` equal
  to `max_height`); every other screen is as tall as its tallest column. On
  the stores screen the card in the right column is as tall as its item, so
  the buttons under it and the whole centred panel move with every pin.
- **Sound and motion.** `combat_audio` has one table of cues, all combat and
  interface. Survey, ride, harvester and bench have no sound of their own and
  no world effect; the bike has a bob and a lean.
- **Abilities.** Every row has a name, an icon and a `says` line, and the
  sheet has a Loadout tab, reached by C and then a button at the foot of the
  professions column. The card follows keyboard focus only, so a mouse sees a
  description only by clicking, which is also "put it there". The HUD bar
  shows no names.
- **Combat effects.** Home made in `combat_fx` (`Pulse`: glow, ring, shield),
  hung on `ShotFired` and `CombatImpact`. No shake, no effect for a blast, a
  status, a guard break or a cut's arc. No particle crate.
- **Crafting list.** Five fixed groups by product kind; MODS is one flat
  group of sixteen; untaught rows are always listed, two lines tall; no
  filter. 31 schematics ship.
- **Icons.** Weapons and their parts have baked pictures. Everything else
  (components, lots, kits, tools, blueprints, the bike, the harvester, bags)
  is slotted's stand-in shape. `follow_registries` hands slotted any asset
  path, and the committed library has tintable symbols for materials, parts,
  schematics and packs, and painted squares for repair, survey and
  extraction.

## The plan, a step a workflow

### Step 1: the screens say what they do

1. **Weapons anywhere.** On the pack screen a weapon in the pack or an open
   bag goes to the hand by T (the quick move, which has nowhere else to send
   it there) and by a click on a hand cell with that weapon on the cursor
   (`equip` reads the cursor: a weapon carried there is the one taken, and
   the hand's goes to the cursor's home cell). The hint and the refusal say
   so. A swap is refused while the member is not Ready (`Busy`), as a fit is.
   The store's weapons stay a locker's.
2. **One frame a screen.** Every screen's root is `min_height` equal to
   `max_height`, as crafting's is. The stores' right column holds the card in
   a scrolling region of fixed height with its buttons at fixed places under
   it (greyed, never removed); fitting's columns and its well are fixed and
   scroll inside; the sheet's details panel the same. Nothing outside a
   scrolling region moves when the pin or the focus does.
3. **The loadout is found and read.** L opens the sheet on Loadout, the
   controls card and the pause menu name it, and Professions and Loadout are
   two tabs at the top of the sheet. The card follows the pointer as well as
   the focus. The known list is followed by what the member's trees teach
   and they have not learned, greyed with the box that grants it. An empty
   place on the HUD bar of a member who knows something to put there shows
   the key that opens the Loadout, and a square shows its ability's name
   under it for a moment when the weapon is drawn or the bar changes.
   Learning an ability says where it was put.
4. **The crafting list.** Chips over the list: Known (the default: what the
   member at the bench was taught), Ready (can be made now from what is in
   reach) and All. Groups: Weapons, Gun parts, Blade parts, Components, Tools
   and vehicles, Kits, each headed with its count; a row is one line (name,
   mount word, the ready mark) and an untaught row's reason is in the right
   column when it is shown.

### Step 2: icons

An item's `icon` may name a symbol of the library (`symbol: "veyra:icon..."`),
drawn where it has no baked picture and tinted with the item's colour: the
nine material symbols by a lot's class, `crafting.component` and
`module.cell` for parts, `crafting.schematic` for a blueprint,
`equipment.pack` for a bag, the painted squares for the repair kit, the
survey tool and the harvester. The bike's icon is rendered from its baked
model by the weapon rasteriser's side-on canvas. The crafting lists and the
card draw the same icon. A list of what still wants an icon of its own goes to
the user for Codex.

### Step 3: the map

M opens a map: north up, the member in the middle, three scales. The ground
is sampled off the frame into an image (height shade over a biome tint, water),
with the baked roads, rivers and lakes drawn over it; sites and Port Serein
with the library's map symbols and their names; deposits only once found, as
a disc in their class colour with their reading, the tracked one and the lead
marked; harvesters and the bench places. Pointing at a mark names it, and
Enter or a click on a found deposit tracks it.

### Step 4: sound, motion and effects

- **Sound.** Cues of their own, made with the forge pipeline the combat
  sounds came from: a survey ping and its answer, sampling (a loop and its
  end), the bike (a hum that follows speed, mount, dismount, thrown), the
  harvester's hum, a bench roll and its outcomes, a fit, a sale, the map.
- **Motion.** A ping is a ring that runs out over the ground to its range; a
  sample is a kneel and sparks at the hand; the bike throws dust by its
  speed; a harvester has a moving part.
- **Combat.** In the `Pulse` system, no new crate: a cut's arc, a blast's
  ring, a guard's spark and a break's burst, a status's glow on its bearer, a
  death's scatter, dust on a dodge and a landing, and a small camera shake on
  a hurt and a heavy hit. The "Blocked" word that never showed in the
  bulwark's stills is found.

## Decided for the user and easy to change

The keys L and M; T as the take key; Known as the crafting default; that every
site and road is on the map from the start and only deposits are discovered;
the order of the steps (the cheap clarity first, then icons, the map, and the
feel last because its sounds take the longest to make).

## As built

### Step 1: the screens say what they do

- Sim: `equip::find_for_take` reads the cursor first; a weapon carried is the
  one taken and the hand's old one goes to the cursor, else to the cell as
  before. `SameFamily` holds from the cursor, other stock on it is `Busy`,
  and `EquipRefusal::MidAction` (the `Combatant` is not Ready) is asked in
  `can_take`, which the screen asks too (no `Combatant` counts as ready).
- Gestures: T on a pack or bag weapon takes it in hand on the pack-only,
  on-foot screen (mounted, in the saddle or at a locker it moves as before);
  a click on a hand with a weapon carried pushes the same take, anything else
  carried there is refused in new words. A take rebuilds the screen.
- One frame: every screen root has `min_height` equal to `max_height`. The
  stores' card is in a `Scroll` of definite width over a foot of fixed height
  (Send, caption, Take, note, blanked with `Visibility::Hidden`, never
  removed). Fitting: fixed columns scrolling inside, a fixed-height well, the
  middle column a plain panel so its parts list is the one thing that scrolls
  (slotted keeps a scroll's direct children from shrinking). Sheet: details
  scroll above Learn and Forget. Bench, board, harvester, survey: min height.
- Loadout: L (unbound before) opens it, C opens Professions; two tabs in one
  strip; the card follows the pointer; `ability::unlearned` feeds NOT LEARNED
  (a hand-granted ability is not repeated). The bar draws `[+]` on a place
  that could be filled, "L: abilities", and names for `NAME_SECS` (4 s).
  The learn toast says "On Q." or "Press L to give it a place."; it follows
  the bar's first change after a cue, so system order does not matter.
- Crafting: Known (default, reset on each workbench F), Ready, All chips; six
  groups with counts; one-line rows; a shown schematic the chip hides falls
  back to the first listed row. `--crafting-filter known|ready|all`.
- Stills in `docs/design/polish/`: `p1-pack-only` (T hint, Take, note),
  `p1-pack-lot`, `-weapon`, `p1-sheet-a`, `-b` (two boxes, one frame),
  `p1-fitting-carbine`, `-blade`, `p1-loadout`, `p1-bar`, `p1-crafting-*`.

Decided for the user and easy to change: the `[+]` mark; names for 4 s; Known
resets on each F.

Left rough: the carried-onto-hand take returns the old weapon to the pack
rather than leaving it on the cursor; a busy crafting row is a disabled
button with no reason said; `p1-bar` shows no names (they had lapsed by the
frame); bench, board, harvester and survey have no layout test of their own;
pointer-follow is tested by a harness hover only; slotted's raised-card layer
was not read, only shown not to move the layout.

### Step 2: icons

- The field: `IconSpec.symbol` (an item file) and `ClassDef.symbol` (classes.ron),
  ids of the committed icon library, checked at load by
  `ItemCatalogue::check_symbols`. A lot's item takes its class's symbol and
  colour.
- The rule: `strata_registry::item_image(def, pictures, vehicles, icons, px)`,
  pure: a baked picture's icon (the bike's own, from `VehicleModels.icons`),
  else the symbol at the smallest export of `px` or more, else `None`, the
  stand-in shape. It says whether the image takes the item's colour: a
  painted square never does. Every drawer asks it: cells and
  `PicturedIcons` (`stores::follow_registries`), the card (`card::item_art`,
  `ItemImages`; a weapon falls back to it when nothing is baked), the
  crafting and factory swatches, the board's order and sale rows, the
  fitting screen's parts. A test walks every shipped item.
- Table: weapons their family symbol (blade for service blade and glaive,
  heavy-tool greatblade); modules their mount's (blade parts and support:
  component, stock); components component (hull plate ingot, cell core
  cell); seal set batch; blueprint schematic; bags pack; bike transit until
  baked; kit, tool, harvester the painted field-repair, survey-pulse,
  extract-core. Classes: reactive reactive, brine liquid (a fluid),
  structural_metal ore, conductor conductor.
- The bike: `veyra_weapons::picture::side_picture` (any meshed building),
  `content --vehicles-only` bakes `icon.courier_bike.png` (median tone lifted
  to 225) and names it in `models.ron`; a test holds it non-empty and wider
  than tall. `--give <item>,..` fills the pack for stills.
- Stills in `docs/design/polish/`: `p2-pack`, `p2-pack-unbaked`, `p2-crafting`,
  `p2-card-lot`, `p2-card-bike`, `p2-card-weapon-unbaked`, `p2-board`.

Wants an image of its own (for the library's author): courier bike (a hover
bike); reactive lots, salt and sulphur each (not a battery); field harvester,
repair kit, survey tool (objects, not ability paintings); drive coil, harness,
mount blank (each its own part); yard greatblade, pier glaive (heavy-tool
reads as a pistol); seal set (a gasket); hull plate (a plate); haul pack
(unlike the satchel); one lot glyph a type (ferrous against light alloy ore,
basin against coastal brine); the three melee parts.

Decided for the user and easy to change: a baked picture wins over the symbol
in a cell, though a long gun's baked icon is a thin line (preferring the
symbol for weapons is one arm of `item_image`); haul pack is a deeper orange.

Left rough: pairs share image and colour (the two ores, the two brines, salt
and sulphur, unbaked feeds, optics and stocks); lists ask for symbols only
(`swatch_of` ignores baked pictures); the harvester screen draws no item image
(it shows a bar, not an item); the bike icon is small on its card;
`Tuning::tone` in veyra-weapons names the bike's slots.

### Step 3: the map

- `strata_sim::chart` (no render type): `Chart::square` (`to_pixel`,
  `to_world`, `unit`, `touches`), `ground` (a sample a pixel: biome tint under a
  hill shade from the north west, sea by depth, lakes, shore line, then rivers
  and roads over their casing), `site_marks`, and `marks` (sites, found
  deposits, the tracked one, the lead, harvesters). Colours are named constants.
- North is `+z` and up, and east is `-x` and right: the world is right-handed
  with `+y` up, so `+x` lies to the west seen from above. `Compass::of` says it
  the same way (East is `-x`), and `Facing::compass` in the ping grid follows.
  The first build had it mirrored; a test holds `+x` left of centre.
- `strata_client::ui::map` (kind `strata:map`), M or the pause menu's Map.
  Scales Near 3 km, Region 12 km (the default), Far 48 km; 512 px. `ground` and
  `site_marks` run on the compute pool and land in a kept picture a scale: 0.41
  s, 0.45 s and 0.52 s at 512 px in release with the water prepared, a few
  seconds in debug. The screen opens at once with "Charting" until one lands,
  and a picture is remade past a quarter of its span from its centre.
- Marked: settlements and sites (names at Near and Region), found deposits as a
  disc of their radius in the class colour with their reading, the tracked one
  ringed or, off the square, an edge arrow with its metres, the lead,
  harvesters, workbenches. Nothing of an unfound deposit; the tracked one is
  always marked, as the register lists it. Names are laid out so none covers a
  mark or an earlier name; one with no free side is left to the panel.
- Focus runs in reading order and the pointer lights a mark; the 176 px panel
  says what it is and Enter or a click on a deposit sends the register's
  ToggleTrack. `deposits::register_row` is the one row both lists use.
- Stills (`--open-map near|region|far`) in `docs/design/polish/`: `p3-map-near`
  (a harvester, Mardle scar tracked), `-region`, `-far`, `-panel` (a mark's
  panel untracked), `-charting`. Found while looking: a working harvester's
  `carry` rebuilt the screen every frame; only what a mark shows is compared now.

Decided for the user and easy to change: default scale Region; the spans and
the 512 px picture (named constants in `ui::map` and `chart`); a site's symbol
(town settlement, hamlet home, landing transit, the rest camp).

Left rough: no harvester or bench mark has a name beside it; at Far the
deposits are small among the site symbols; the ping grid's compass words
changed with the East fix; `ui::survey::paint` has the swatch ordering race the
map's `paint_swatches` was ordered after the theme to avoid.

### Step 4: sound, motion and effects

- Sounds are arms of the one table in `combat_audio.rs`, hung on events the
  sim raises (never a key): ping (`Added<Pinging>`), found and empty (the
  `PingAnswer` by its result), sample loop (while `Sampling`), sample done (a
  `SampleAnswer` cycle), bike call (`Added<Calling>`), bike hum (while
  `Riding`, pitch 0.8 to 1.35 by speed), mount, dismount, thrown (`RideNotice`),
  harvester hum (`LoopEmitter`, within 60 m, on while not full), plant and
  collect (`HarvestAnswer`), weapon swap (`Action::Swap`), guard break (impact
  `Broken`), blast, craft roll and outcome by grade, finish, fit, repair, sale
  (answers), map open (in place of the plain open). The loops end near zero and
  need no crossfade.
- Motion: a ping is a standing band that runs out to its real range over its
  channel (a flat ring was invisible from the play camera); a sampler kneels
  (`Kneel`, the IK stands aside) with sparks every third of a second; the bike
  throws dust by speed, none standing or over water, and a glow; a running
  harvester's plate pumps with a puff at each stroke.
- `Pulse` effects: a cut's arc (Heavy wider), Blocked sparks, Parried flash,
  Broken burst, a blast's ring, a death's scatter, dodge and landing dust, a
  breathing glow per status.
- Shake: `Feedback.trauma` (a hurt by its pool share, a heavy hit a little,
  0.4 s decay, 5 cm); Display's Camera shake turns it off. Cap: `PULSE_CAP`
  192 transient pulses, oldest first, counted in `PostUpdate`.
- The missing Blocked word: the still's `--hold-fire` had no aim, so its bolts
  never met the slab, and a held trigger stacked a word per bolt in one
  smear. `--hold-fire` aims at the nearest hostile; words now merge (the test
  guards the smear, not the aim). Stills: `docs/design/polish/p4-*.png`; no
  sound was heard.

Decided for the user and easy to change: presentation figures are consts in
the client, not data files; the blast is inferred (no sim event).

Left rough: the blast inference misses point blank shots (a sim Blast message
is the fix); the arc ignores baked clip curves; the hum's volume is not tied
to speed; no Heavy arc, bulwark break or player status still.
