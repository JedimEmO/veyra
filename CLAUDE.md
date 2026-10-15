# Veyra

Veyra (this checkout is still named `sci-fi-scrolls`) is a Bevy 0.19.1
game engine for a sci-fi MMO-like RPG, built as a workspace of layered
crates with a thin game on top. The architecture is `docs/PLAN.md`
and the decisions in `docs/adr/*.md`; read them before changing anything
structural.

## The layering rule

```
strata-core <- strata-registry <- strata-worldgen <- strata-flora    no Bevy at all
                                     strata-worldgen <- strata-weather  (where fog lies; the server links it)
strata-sim <- strata-stream <- strata-physics           bevy_app, bevy_ecs, bevy_math, bevy_state, bevy_time, bevy_transform, bevy_tasks
                                                           (physics adds avian3d and its bevy_log, bevy_heavy, bevy_transform_interpolation)
strata-net (port) <- strata-net-replicon (adapter)       above physics, below the render crates; replicon and renet, no socket in the sim
ashlar (buildings), veyra-buildings (the game's own kits over it), veyra-weapons   no Bevy either; `ashlar-manifold`, the native mesher, is authoring only
veyra-materials, veyra-flora, veyra-rocks, veyra-settlements, tools/content   authoring only: ashlar-content, ashlar-material, ashlar-impostor and the kernel
tools/balance                                              the duel harness: strata-sim, strata-stream and strata-physics, no render crate
strata-terrain, strata-atmosphere, strata-foliage, strata-client, ashlar-bevy   bevy_render and friends (client's `ui` feature adds bevy_ui and slotted)
strata (facade), apps/game, apps/server, apps/worldmap    see everything (the server only the headless feature set plus `net-replicon`)
```

An arrow never points from a lower crate to a higher one. `ashlar` and
`veyra-buildings` are Bevy-free and may be used from the physics layer up;
`ashlar-bevy` is a render crate; `ashlar-manifold` carries the native Manifold
kernel and, with `ashlar-material`, `ashlar-content`, `veyra-materials` and
`veyra-flora`, `veyra-rocks` and `veyra-settlements` (the plant, rock and
city generators), belongs to `tools/content`, the preview tool and tests alone, never to
`game`, the server or the headless facade, which `just server-check` asserts
by name. `just content` bakes buildings, materials, trees, the tree
library and each settlement's layout and skyline into `generated/`
(gitignored, regenerated, never edited), and the game mounts it as the
`generated://` asset source; the game reads baked files and bakes nothing. Crates below
`strata-sim` have no Bevy. `strata-sim`, `strata-stream`,
`strata-physics` and `strata-net-replicon` use the non-render Bevy
crates listed above and nothing else. Only crates above them see
`bevy_render`. `just server-check` asserts that exact allowlist on the
facade's headless feature set and a render/window/UI denylist on the
server binary's graph, then builds and tests the lower crates on their
own (feature unification in `cargo test --workspace` would hide a
render-side use in them), and is part of CI.

Positions below the render layer are `WorldPos` (`f64`); `Transform` is
derived from them through `RenderOrigin`, never the other way (ADR 0002).
Physics runs in `f32` render space and its bodies are rebased with
everything else. Vegetation is placed per 64 m cell by `strata-flora`
(no Bevy) from the seed, the species registry and the baked tree library
(`generated/flora/library.ron`, versioned and part of the replication
handshake); the physics crate gives every placed tree the library's trunk
collider and `strata-foliage` draws the same placement: near trees as
entities per baked mesh level, far trees as per-cell impostor batches
(ADR 0005). Reeds are the one dressing species, `assets/flora/reed.flora.ron`
(`FloraDef::dressing`), placed only on a lake's or a wide river's wet bank and kept
out of the world's version and the server's placement, so only the client draws
them. Where fog lies is `strata-weather` (no Bevy, ticket 0003):
`BasinTile::bake` reads one 8 km tile of ground into where cold air pools
and to what level, once, off the frame; `FogField::sample` turns baked
tiles and a `Weather` into a fog layer's top and density at any point;
`just worldmap` draws it as the `-fog` layer. The ground's geology is
`assets/geology/*.geology.ron`, a province a file (ADR 0011, ticket 0004):
`strata_worldgen::geology` picks a province per point by a field of its
own, independent of the climate, `Sample::provinces` carries the pair, the
terrain draws a province's rock on bare faces and placement picks its rock
kit by it; `just worldmap` draws the `-geology` layer. Rivers and lakes
are `strata_worldgen::hydrology` (ADR 0010, ticket 0005): the network
baked per land mass, the carve that cuts their valleys into every sample,
and `Sample.water`, the record at the point (its `cut_dm` is how far the
carve lowered the ground, which marks a valley's cut wall for the terrain's
look and the placement). The water a character is in is
`strata_sim::water_here` (the sea, or a river or lake standing higher
there), which the character step in `strata_physics::character` reads
once a tick for wading, swimming and the current, the same on a server;
its tunables sit on `CharacterController` beside `swim_factor`. River and
lake water is drawn by `strata_terrain::river`: a small mesh per terrain
region, built on the region's meshing task from its own samples, on a
child of the region's entity, shaded by `river_water.wgsl` over the sea's
`water_common.wgsl`. Rivers and roads past the near drawing are painted by
the terrain's fragment shader from each region's packed lines
(`strata_terrain::paint`, ticket 0005 step 8a), and `--no-far-paint` compiles
that shader without them. The falls over the river's ramps are `strata_terrain::falls`: a sheet, foam and spray per fall, built from the baked record alone, streamed by the water plugin to 18 km and shaded by `fall_water.wgsl`, with `--no-falls` leaving them out. The river network is `strata_worldgen::hydrology` (no Bevy, ADR 0010, ticket 0005): one bake
per land mass on a 512 m lattice, lazy and kept, whose numbers and three
canary networks join `world_version` (a change to it that moves rivers is a
`GENERATOR_VERSION` bump); the shape (`hydrology/shape.rs`) settles each
reach onto its valley floor and gives it meanders, whose winding channel
`River::centreline` gives with its width and depth (a ford is
`FORD_DEPTH_M` or shallower); the carve (`hydrology/carve.rs`) is the last
height stage of every sample: valleys, channels and lake beds cut into the
ground, and `Sample::water` carries the river or lake at the point
(`Sample::in_water` under its surface), so colliders, placement and the fog
follow; `ProceduralWorld::water_at` reads the network alone. A sample that
meets an unbaked mass bakes it on its own thread, so the game and the server
bake round the spawn and the settlements before their app starts
(`strata_sim::prepare_water_before_start`, `PrepareWater`, 128 km) and the
loader bakes ahead of the observers on two threads as they move; a test
world prepares its own (`prepare_water`). `just worldmap` draws the
`-rivers` layer. The roads (ADR 0012, decision 7; ticket 0005, steps 5 and
6) are baked in the same bake, after a mass's sites, by
`hydrology/roads.rs`: a least-cost network of service roads, tracks and
trails with its junctions and its river crossings
(`strata_worldgen::roads`, `Network::roads`, `nearest_road`), Port
Serein's own road kept as its first link and left to the settlement; each
service road's and track's profile is fitted there too
(`hydrology/grade.rs`). The sample grades the ground in its last stage
beside the carve, from the same cell (`RoadGrid`): the carriageway, verge,
cut and fill banks and a bridge's gap, recorded in `Landform::cut_m` and
`cleared`; `sample_without_roads` is every stage but that. `just worldmap`
draws the `-roads` layer (`--no-roads` leaves it out). The terrain's
regions are not `Mesh` assets: the meshing task packs each into a record
that `strata_terrain::store` writes into paged storage buffers, and every
region draws one shared grid mesh with its slot in `MeshTag`
(`docs/perf/streaming-2026-10-04.md`). `strata-atmosphere` draws
the fog: a clipmap of the field (8 km at 32 m, 64 km at 250 m, built on the task
pool) that a full-screen fog pass marches through after the transparent
pass; the benches run with `--fog 0`, which registers none of it. The sim
owns the player: input writes a `PlayerIntent`, the tick consumes it into
a `PlayerCommand`, the physics moves the character in `SimSet::Step`; a
server does exactly the same with intents that arrived over the wire (ADR
0004). Progression (`docs/design/progression.md`, section 9): the skill
graph is `assets/progression/trees.ron`, loaded and checked by
`strata_registry::progression` (no Bevy); the `CharacterSheet` component,
the learn and forget rules, the merge `apply(tables, graph, sheet)` and the
XP ledger are `strata_sim::progression`; a fitted `Combatant` reads its
own rows through `Combatant::spec` and friends, never `Action::spec`
directly, and one never fitted reads the global tables live. The world
side (section 10) is `apps/game/src/household.rs`: the `Household`
resource (the bench of four, who is out, who can still join), saved as
RON to `--household` (default `scratch/household.ron`) on every change
and read back at startup (`--fresh` ignores it); `Interaction` props
(a terminal, a blade and a marksman trainer, at the staging spawn and on
the Exchange) that `strata_sim::progression::find_reach` measures into
the `Reach` resource, which is the trainer rule's `teacher`; the client's
F raises `Interact`; the swap retints the body through
`strata_client::combat_art::BodyTint`. The screens (section 11) are the
client's, on the slotted stack beside the pause: `strata_client::ui::sheet`
(key C, a trainer's F, the pause's Character button) rebuilds its
definition from the local player's sheet, the graph and the reach on
every change and calls `learn` and `forget` itself; `ui::bench` (a
terminal's F live, the pause's Bench button read-only) shows the
`BenchView` the game publishes from its `Household` and sends
`BenchRequest::{Switch, Join}`, which `household::answer_bench` answers
with a `BenchOutcome` toast; a switch runs behind the client's fade.
`ui::prompt` is the F chip on the HUD, `ui::progress` the XP toasts and
the box hint. `--script-progression` walks the whole path headless
(learning and swapping directly) and the test in `household.rs` runs it;
`--open-sheet` and `--open-bench` open a screen on frame 120, or, with
the script, at the trainer and the terminal, for the screenshots in
`docs/design/progression/`. Local-only: neither the sheet nor the
household is replicated and the protocol was not touched. Learn, forget
and the bench switch are written to the sim's components by the screen
and the game directly, not through a `PlayerIntent` message consumed in
the tick; moving them onto that path is the next structural step before
a server can own them (progression.md, section 12).

Every combat number is `assets/combat/*.ron` (actions, weapons,
actor, opponents), loaded by `strata_sim::combat_tables` into one
process-wide slot (`install`, `current`) that `Action::spec`,
`Weapon::spec` and `Combatant::step` read; the game and the server load the
directory at startup and fold `CombatTables::version` into the replication
handshake, so a client on other numbers is refused. The encounter's
opponent AI is `strata_sim::combat_ai`, which the game and `tools/balance`
both run. `tools/balance` (`just balance`) fights policies over the real sim
and physics headless, sweeps a grid of table values and checks the feel
contracts of `assets/combat/contracts.ron` (`just balance-check`, and the
`contracts` test in `just test`); `docs/balance/README.md` is the procedure.

Procedural sites (ADR 0012, ticket 0005 step 4): site kinds are
`assets/sites/*.site.ron`, loaded and checked by `strata_registry::site`;
the bake places them per land mass after its rivers
(`strata_worldgen::hydrology::place`), deterministic on any thread count
and hashed into `world_version`, and each lays a level pad in the sample
(`strata_worldgen::sites::Pad`, developed, gravel only in its yard).
`just content` bakes a building group per kind into
`generated/sites/<ns>/<group>.ron` and its files; `strata_sim::sites`
reads them, keeps `NearSites` round the observers and stands each group
with `place_group`, a pure function of the site and the layout, from
which the physics builds colliders (server and client) and the game draws
(`apps/game/src/sites.rs`); the handshake carries
`site_groups_version`. The site's name is the client's HUD card
(`ui::place`). `just worldmap` draws the `-sites` layer. The roads'
crossings (ticket 0005 step 7) are the same pattern: the bridge kit is
`veyra_buildings::bridge` (span classes, piers, culvert headwalls, ford
posts, stepping stones), `just content` bakes it into
`generated/crossings/veyra/` with its layout `kit.ron`;
`strata_worldgen::crossings::place_crossing` stands it at a network
crossing from the record, the layout and the ground (reporting
`Unplaced` rather than a deck in the air), `strata_sim::crossings` keeps
`NearCrossings`, the physics collides it and `apps/game/src/crossings.rs`
draws it; `site_groups_version` hashes the kit too. The roadside kit (ticket 0005
step 8c) is the same again: `veyra_buildings::roadside` bakes a marker post and a
lamp to `generated/crossings/veyra/roadside.ron`, `strata_worldgen::roadside::place_roadside`
stands them along the roads, `strata_sim::roadside` keeps `NearRoadside`, the physics
collides them and `apps/game/src/roadside.rs` draws them.

Weapons (ticket 0008): `veyra-weapons` (no Bevy) is
the weapon recipes, one ashlar part for the chassis and one per mount in a
shared frame, with what fits what; `just content` also bakes each piece into a prop (below).
`veyra_weapons::picture` rasterises a meshed weapon side on (muzzle right) into
a luminance and alpha picture per piece, a mount's piece drawn with the chassis
as its occluder so the pieces stack into the weapon; `just content` bakes them
to `generated/weapons/veyra/<chassis>/*.png` with the index
`strata_registry::pictures::PictureSet` (`pictures.ron`: each piece's
rectangle in its chassis's frame, `hidden` and `same_as` where a picture says
nothing, and the square `icon` baked beside every picture as
`icon.<stem>.png`), and `just review` writes a contact sheet a maker to
`scratch/review/weapons/`. Client art only: not in `world_version` or the
handshake. The client draws them with `strata_client::ui::weapon_diagram`: the
`WeaponPictures` resource the game fills at startup, the slotted widget
`strata:weapon_diagram` (pieces placed by their rectangles, tinted by state
from `strata.weapon.*` roles, each mount a button that writes `MountPicked`,
the mount in focus bracketed and named on a tag in the frame's lower right
corner, the well narrowing to a weapon held
to its `max_height`, labelled boxes when nothing is baked), which the fitting
screen draws its weapon with. A cell of a grid draws an item's icon where the
index has one (`ItemCatalogue::frozen_with`, `ui::stores::follow_registries`)
and slotted's stand-in shape where it has not.
`just weapon-workbench` is the standalone assembly studio.
The weapon in the hand (ticket 0007, step 8): `content::weapon_models` bakes
every chassis, standard part and module to `generated/weapons/veyra/<chassis>/
<stem>.ashprop` (one level, no collider, materials `weapon:<maker>.<slot>`,
a plain library `weapons/veyra/library.materials.ron` of `Issued` colours) and
the index `strata_registry::models` (`MODELS_PATH`, with each muzzle's
`veyra_weapons::model::muzzle_tip`). `strata_client::weapon_model`
(`WeaponModels`, `wanted`, `frame`) names the pieces a record wants; `combat_art`
`dress_weapons` stands them as `AshlarProp`s under a gun's root once the
actor's `Hands` change and the wanted set differs, `settle_models` swaps the
loaded model in (a failed load keeps the old art), `hide_old_art` hides the
glTF rifle and cubes, and the muzzle marker follows the tip. No `Hands` or
no baked files keeps the old art. Not in a version or the handshake.
`--orbit-distance <metres>` sets the combat camera's boom for stills.
The fitting screen's well draws the weapon as a turning model
(`strata_client::ui::weapon_stage`): a `WeaponStage` resource (a 1024x512
target image, there when models are baked) and, only while a fitting screen
in Model view is on top, one extra `Camera3d` and two lights on
`STAGE_LAYER`, released when it leaves. The widget is `strata:weapon_model`;
a button in the well switches Model and Diagram (`WellView`, Model first),
`--fitting-view model|diagram` is for stills. The stage camera carries
`NoSky` and `strata_terrain::OffWorld`: a new system over cameras must
filter `Without<OffWorld>` (or rely on `NoSky`) so it leaves the stage alone.

Items (ticket 0007, step 1): item definitions are `assets/items/*.item.ron`
with `carry.ron` (the sizes of the pack, the bags and the store) and
`starting.ron` (the first day's stock), loaded and checked by
`strata_registry::item` into `GameRegistries.items` (`ItemCatalogue`), in no
world version and no handshake. A record is a `slotted_model::ItemStack`
whose patch holds the batch, the maker, a quality figure per line and, for a
weapon, its condition and what is fitted; `strata_sim::items::{read, write}`
is the only reader and writer. The sim owns the stock: `Pack` (a grid, two
bag slots whose bag opens its own cells, the cursor) and `Hands` on the
player, `HouseholdStore` as a resource, and a click is a `StockRequest` put in
`StockInbox`, consumed in `SimSet::Input` by replaying slotted-model's click
rules on the truth and answered with an `Ack`, a `Resync` or a
`StockRefused` with its reason. The game saves them in the household file
(version 2; an older file is upgraded and kept beside as `.v1.bak`). The
client's `ui::stores` is a mirror through slotted's `Authority` port
(`StockAuthority`) and decides no rule: a locker's F (`InteractionKind::Locker`)
opens the store beside the pack, key I and the pause's Inventory button the
pack alone, and `ui::card` paints the one item card for the pinned pane and
the hover. `--open-stores` and `--open-inventory` open them for a still. The
`Combatant` still keys its tables by the `Weapon` enum, which an item names as
its family; a hand holds any weapon, never two of one family (step 2 below).

Fitting (ticket 0007, step 2): a module's effects are data on its item file
(the property, the authored factor, the quality line that drives it; the
factors are `veyra_weapons::MODULES`', which a `tools/content` test holds
them to) and a weapon's file lists its mounts with the moddability each
opens at. `assets/crafting/fitting.ron` holds the bands a quality line moves
a factor in, how a magazine is made whole and which way each property is
better (`strata_registry::item::FittingRules`). `strata_sim::fitting` is
every rule and every figure: `live` says which properties the fight has a
number for (the rest stay in the data and are shown as not in the fight
yet), `loadout` and `merge` put the fitted factors on a weapon's row after
the sheet's boxes (`Combatant::equip`, `bare`; an actor with nothing fitted
reads exactly what it read before), `mounts`, `candidates`, `with` and
`lines` are what a screen shows, in the game's units with the change
already worked out, `at_workbench`, `without`, `preview`, `bare_of`, `host`
and `lead` are the rules a screen asks instead of writing them again, and a
fit or a removal is a `FitRequest` put in
`FitInbox`, consumed in `SimSet::Input` after the stock's own, allowed with
a workbench in reach (`Reach::workbench`, `WorkbenchAccess`) and answered
with a `FitAnswer`. A workbench (`InteractionKind::Workbench`) stands one
spacing past the locker at each bench place. The client's `ui::fitting`
(kind `strata:fitting`) builds a `FittingView` from those functions and
decides no rule: a workbench's F opens it live, a weapon's cell on the
stores screen opens it from anywhere as a reading, focus or the pointer on a
part previews it in the AFTER column, and `ui::card` shows a module's lines
against the weapon carried (`card::Against`, `CardContext`). `tools/balance`
takes a loadout on a side (`--fit`, `--b-fit`). `--script-crafting` walks
the chain headless and `--script-report` writes what the `Combatant` read;
`--open-fitting [mount]` opens the screen for a still (`--fitting-hand`,
`--fitting-pick`) and `--fitted <item>` starts with a module on its weapon.
Spread (step 10): a weapon's row gains `spread` and `aim_spread`, half
angles in degrees (hip, aiming); `combat::deviate` turns a bolt inside the
`Combatant::cone`, drawn by `combat::shot_draw` from the shot count
(`shots` plus `shot_salt`, set only by balance bouts), team and weapon.
`AimSpread` is live (`merge` scales `aim_spread`, shown in degrees); the
reticle's gap is the cone via `combat_hud::cone_gap`, never under the old
gap. Step 11 (tuning): the cones are 2.4/0.9 (carbine) and 2.8/1.3
(pistol; widened the same day to 4.0/2.0 and 4.4/2.4, weapons.ron version 4, so
the reflex sight shows at the sentry's 27 m, and a harness is made by
`veyra:schematic.harness` from a conductor, which lifts a made carbine's Action
line past the 500 a bought one is stuck at), the reflex sight is AimSpread 0.70, fitting.ron's bands run 0.4
to 1.6 of a benefit and 1.5 to 0.5 of a drawback, and a weapon's own
`signature` (`ItemKind::Weapon`, `registry::item::Signature`) is a line that
multiplies one number of its own row: the patrol carbine's `interval` runs
1.30 (line 0) to 0.90 (line 1000) of the table's, so a bought one fires at
1.10 and `fitting::factors` adds it after the parts and before the wear
(`WeaponMeans::Row` words it in the crafting tab). `balance --line id=n`
puts a record with that line on a side; presets `bought-` and
`made-carbine-vs-dummy` and the `aimed-`, `sighted-` and
`fine-sighted-carbine-vs-dummy-55m` trio hold the four new bands.

Resources (ticket 0007, step 3): the class tree and the resource types are
`assets/resources/` (`classes.ron`, a `*.resource.ron` a type, `deposits.ron`
with every dial), loaded and checked by `strata_registry::resource` into
`GameRegistries.resources`, in no world version and no handshake. Where
deposits are is `strata_worldgen::deposits` (`DepositField::{near, get,
nearest}`), a pure function of the seed, that data and the samples: not in
the hydrology bake, not in `world_version`, and a deposit's id is its
lattice cell. A deposit is an area (ticket 0009, step 1): `radius_m` and
`peak` roll per cell inside `deposits.ron`'s `area`, `Deposit::concentration`
is the per-mille at a point, `DepositField::covering` every deposit holding
it and `strata_sim::deposits::concentration_here` asks it. `LotOrigin`
reads past an old `terms`. `just worldmap` draws the `-deposits` layer as
discs. A lot is a stack: the catalogue makes an item a type
(`strata:lot.<type>`, kind `Lot`) and a record's `origin` names its
deposit, so lots of two deposits never merge. Surveying is by hand (ticket
0009, step 3): `assets/resources/survey.ron` holds the ping, sample and
reading dials, and the carried tool `veyra:tool.survey` (`ItemKind::Tool`,
a `range` line) scales the ping. `strata_sim::deposits` holds `Survey` (the
field), `NearDeposits`, `Purse` and `Surveyed` (`found`: what each deposit's
`Reading` is, whether sampled, the best reading; `tracked`; `lead`), with
Ping, Sample, Stop, Track and ToggleTrack as `SurveyRequest`s in
`SurveyInbox`, consumed in `SimSet::Input`. `deposits/hand.rs` has `can_ping`
and `can_sample` (one rule for request and screen), the `Pinging` and
`Sampling` channels, `ping_with`, `PingResult` (`cell_of`, `lead`) and
`LastPing`; a deposit must be pinged before it is sampled, and sampling
reads it Exact. `lot`, `lot_lines`, `best_line` and `register` (found
deposits only, and the tracked one always) are what a screen asks. The game
saves the purse and `Surveyed` in the household file (version 3; an old
`visited` reads as found and sampled; the tool is given once to an old
household), stamped with the world (`SurveyedIn`). The client's `ui::survey`
(G) is the ping grid and Sample here, the terminal's second tab the register
(`ui::bench`), `ui::tracker` the tracked deposit's and the lead's markers,
`ui::card` a lot or deposit by its `Reading`, and `ui::place` names a deposit
once found. Compass words are the sim's `Compass`: north is `+z`. `--at
deposit[:type]`, `--at bench`, `--open-survey`, `--ping <class>`,
`--sampling`, `--open-register`, `--track <type>`, `--stores-filter <chip>`
and `--no-hostiles` are for stills; `--script-survey [class]` walks and pings
to a deposit and samples 15 units, writing `--script-report`.
Crafting (ticket 0007, step 4): a schematic is `assets/schematics/*.schematic.ron`,
loaded and checked by `strata_registry::schematic` into
`GameRegistries.schematics` (what it makes, its complexity, its slots, and a
weighted recipe a line of the product, each weight on a figure its slot
carries), with the session's dials in `assets/crafting/session.ron`
(`SessionRules`: the assembly share, the points by complexity, the roll
table, the workability rule); in no world version and no handshake. Items
gain the kinds `Component` and `Commodity`, a `price` and an optional
`article`. `strata_sim::crafting` is every rule, in whole per-mille:
`ceiling`, `candidates` (best first), `best_stock`, `check_picks`,
`can_make`, `preview` and `now` (each line's floor and ceiling and what they
are worth in the game's units, a part's read through the first schematic
that uses it), `hand_odds`, `can_roll`, `lifts_odds`, `can_buy`, and
`assemble`, `roll_line`, `finish` and `buy` behind the `CraftRequest`s of
`CraftInbox`, consumed in `SimSet::Input` after the survey's, allowed with a
workbench in reach and answered with a `CraftAnswer`. `CraftState` is the
roll counter, the batch counter, the open session and the ledger of what
each batch was made from; the game saves it in the household file (version
4) before the screen hears the sim's answer, so a roll stands once made and
a session left open is there again, and at load `CraftState::repair` holds
it to the data as it now is (a retuned dial, a changed item) with the file
as it was kept as `.mended.bak`, where `check` refuses only what nothing can
work on. The client's `ui::crafting` (kind `strata:crafting`) is the
workbench's second tab and decides no rule: the strip of two tabs both
screens start with (`bench_tabs`, `switch_tabs`, `open_bench`), a
`CraftingView` built from those functions in three states (the slots, the
bench hand, the finished card) with the acting buttons at the right
column's foot, a roll's bar filled in place (`animate_roll`) and a held hand
repeated (`repeat_held`: Accept held on the focus, or a pointer held down,
whose press is the roll). `ui::card` draws a part with where each line goes,
a commodity as flat, and what a made thing was made from (`Against::made`);
the stores have a Parts chip. `--script-crafting` makes a core, the magazine
with it and a second core; `--open-crafting [schematic]`, `--crafting-state
empty|pick|hand|done` and `--craft-first <schematic>` are for stills, and
`hand`, `done` and `--craft-first` spend stock, so they are refused on the
default `--household` (pass `--fresh --household scratch/crafting/shot.ron`).

The factory (ticket 0007, step 5): the dials are `assets/crafting/factory.ron`
(`strata_registry::schematic::FactoryRules`: the derating's range and step,
the runs a blueprint starts with by complexity, the played seconds a unit a
point of complexity, the unattended share), in no world version and no
handshake. A blueprint is the item `veyra:blueprint.production` (kind
`Blueprint`, a stack of one) whose body is in the household's ledger
`strata_sim::blueprint::Blueprints` under the stack's batch number, so the
patch gained no key. `strata_sim::blueprint` is the recording, in whole
per-mille: `draft` (what the Record box shows and what is written),
`check_record` (every refusal, asked by the screen and the request alike),
`record` behind `CraftRequest::Record` with a workbench in reach, `prototypes`
and `held`; the prototype is not consumed and is recorded once.
`strata_sim::factory` is the run, one job a household: `meets`, `feeds`,
`cheapest`, `limit`, `check_start`, `seconds`, `left` and `can_stop` are what
a screen asks, Start, Assign and Stop are `FactoryRequest`s in `FactoryInbox`,
consumed in `SimSet::Input` after the bench's with a fabricator in reach
(`Reach::fabricator`, `InteractionKind::Fabricator`, one spacing past the
workbench at each bench place) and answered with a `FactoryAnswer`; `advance`
makes a unit a stretch of played time counted in nominal ticks
(`nominal_ticks`), with the blueprint's lines exactly, into the household's
store, pausing when it is full (`FactoryNotice`), and a pause read back from a
save keeps its unit. The worker is a benched member by name (`Benched`, which
the game publishes); the game refuses a switch to them (`household::swap_to`)
and the bench card says so (`BenchCard.work`). The game saves `Blueprints` and
`FactoryState` in the household file (version 5, a version 4 file kept as
`.v4.bak`). The client's `ui::factory` (kind `strata:factory`) is a
fabricator's F and decides no rule; Record as blueprint is the crafting tab's
finished card, with MADE EARLIER for a prototype still in stock; `ui::card`
draws a blueprint and a factory stack, and the stores have a Blueprints chip.
`--script-crafting` records its second core and runs three off it;
`--open-factory`, `--crafting-state record` with `--record-derate`,
`--record-first [percent]`, `--factory-run [n]`, `--factory-worker`,
`--factory-finish`, `--factory-full-store`, `--factory-poor-lot`,
`--join-all`, `--at terminal` and `--stores-pin n` are for stills, and those
that spend or change the household are refused on the default `--household`.

Wear (ticket 0007, step 6): the dials are `assets/crafting/wear.ron`
(`strata_registry::wear::WearRules`, `ItemCatalogue::wear()`: where lines
start to slide, what a shot, a cut and a fall cost, the step, the `worst`
factors, the field repair), in no world version and no handshake. A weapon
and a module carry a condition (`Fitted::condition`, absent at full), and a
kit is the item kind `Kit` with a `restore` line (`veyra:kit.field_repair`,
made by a schematic). `strata_sim::wear` is every rule in whole numbers:
`slide`, `band`, `cost`, `field_amount`, `can_repair` and `repair` behind
`WearRequest::Repair` in `WearInbox`, consumed in `SimSet::Input` after the
factory's and answered with a `RepairAnswer`; the tick charges shots and
cuts to the hand's `WearCarry` and a fall knocks everything carried once
(`WearNotice`). The effect joins the loadout only in `fitting::factors`,
and an item above `slide_below` reads exactly as made. The client shows it
on `ui::fitting` (bar, word, Repair), the HUD vitals and `ui::card`.
`--condition <n>` (game) and `--condition`, `--b-condition` (balance) are
for stills and the harness.

Procurement (ticket 0007, step 7): a buyer is `assets/procurement/*.buyer.ron`
(orders of lines: what is wanted, the grade it is read on, a minimum, a
quantity, a price, an Upkeep line's refill), loaded and checked by
`strata_registry::procurement` into `GameRegistries.buyers` (`BuyerSet`),
in no world version and no handshake; one ships, Pelagic Works.
`strata_sim::procurement` is every rule in whole numbers: `wanted`,
`taken_pm`, `grade_of` (a worn kind under `slide_below` is not graded),
`pays`, `pay_for`, `offers`, `best_short`, `at_board`, `check_sale` and
`sell` behind `ProcurementRequest::Sell` in `ProcurementInbox`, consumed in
`SimSet::Input` after wear's and answered with a `SaleAnswer`; stock leaves
the pack then the store, the `Purse` gains the pay and `ProcurementState`
(taken a line, the ledger by buyer, line and kind, the latest sale last)
records it. An Upkeep line refills on played time (`ProcurementClock`,
unsaved); a Project line never. `InteractionKind::Board { buyer }` and
`Reach.board` are the prop, at +2 spacings of each bench place; the game
saves the state in the household file with a serde default, no version
change, and `ProcurementState::repair` holds it to the data. The client's
`ui::board` (kind `strata:board`) decides no rule. `--script-crafting`
sells its run's cores at the board; `--open-board`, `--at board`,
`--board-stock` and `--board-sell` are for stills.

The smith (ticket 0007, step 9): the elite tree `weaponsmith` in
`assets/progression/trees.ron` (Fabrication XP, 98 points) has one new
effect, `Effect::CraftPoints { add }`, on its four tuning boxes. A schematic's
`profession` must name a box that exists (`check_professions`); in
`strata_sim::crafting` `taught` gates `check_picks`, so opening a session
without the box is `CraftRefusal::NotTaught`, while finishing one, recording
and a fabricator's run need none. `crafting::points` is the dials' points plus
the sheet's `CraftPoints`; a weapon is a schematic's product
(`veyra:schematic.patrol_carbine`) made whole via `made_condition`, which
`Blueprint::unit` shares. Its lines read as mounts open, shots to the slide
or not in the fight. `strata_sim::equip` (`EquipRequest::Take` in
`EquipInbox`, `can_take`) swaps a weapon from the pack, or the open store at a
locker, into a hand; the hands stay locked for clicks. The client greys an
untaught schematic with its reason, and the stores' pinned pane has Take in
hand. Tovan Reyes is the smith; `--script-crafting` runs as him and ends with
his carbine in the arrival's hand. `--as <member>` and `--crafted-carbine
<moddability>` are for stills. Rework: `assets/crafting/rework.ron`
(`strata_registry::rework`) names the mount blank
(`veyra:component.mount_blank`, a temper line, taught by
`weaponsmith.frames.2`) and the share of the gap an outcome closes;
`strata_sim::rework` has `check_rework`, `best_blank`, `result_of` and
`ReworkRequest::Rework` in `ReworkInbox` (one roll, one blank, a refusal
changes nothing). Crafting XP is `crafting::craft_xp` to the maker of a
finished bench session, rates in `trees.ron`'s `rates.craft`; `CraftState`
keeps `bests`. A tree's `bench` flag makes `Reach::teaches` true at a
workbench (`TreeDef::in_game` puts it on the sheet). `ui::fitting` shows
Rework and Buy blank live; `--blanks <n>` and `--rework-first` are for stills.

The ride (ticket 0009, step 2): a vehicle is `assets/vehicles/*.vehicle.ron`
(every dial: speeds, turn, slope, water, road, crash, call, view), loaded and
checked by `strata_registry::vehicle` into `GameRegistries.vehicles`, in no
version and no handshake. `strata_sim::ride` holds `Stable` (the
household's bike record and its saddle), `Riding` (the dials copied at the mount, heading, speed, the bike's
velocity), `RideRequest::{Call, Dismount}` in `RideInbox` with `can_call` as
the one rule, and `RideNotice`; a hit, a crash or deep water throws the rider.
The motor is `CharacterController::ride` (`strata_physics::character::ride`),
the same capsule and slide with the bike's own stop angle as its wall. The kit
is `veyra_buildings::vehicle`, baked to `generated/vehicles/`. V calls or gets
off (client `ui::ride`, `ride_art`); `--bike`, `--mounted`, `--script-ride`.
The bike is owned (step 2, second half): the item `veyra:vehicle.courier_bike`
(kind `Vehicle`, lines speed, handling, durability, a condition) is bought at
Pelagic's board (`sells`, `ProcurementRequest::Buy`, `check_purchase`);
`RideRequest::{Stable, PutAway}` make it the ride and `ride::dials` turns file,
lines, sheet and wear into the numbers. Its saddle is `StockLayout::SADDLE`,
open mounted or at a locker; a crash wears it, `WearRequest::RepairBike` mends
it. The file keeps `stable`. Stills: `--purse`, `--saddle-stock`,
`--bike-condition`; `--bike` gives a store grade bike to a household with none.

The surveyor (ticket 0009, step 4): a box may carry `Survey { field, op,
value }`, `Read`/`Peak` and `Ride { field, op, value }` (data in `trees.ron`),
folded Set, Add, Mul by `progression::learned_effects`, which
`crafting::points` shares. `deposits::skill` reads them over the survey dials
and the tool's line; `ride::dials` at the mount and `ride::call_s` the call.
`rates.survey` and `deposits::survey_xp` pay Survey XP (none for an empty
ping or while `SurveyUnkept`). A survey trainer stands at +3 spacings of a
bench place; Ilse Marrow starts as the surveyor.

The fabricator's trade (ticket 0009, step 5): a box may carry `Craft`,
`Factory` or `Wear { field, op, value }`, read through the surveyor's one fold
(`craft_field`, `factory_field`, `wear_field`). The bench sheet reads the
assembly share, `CraftPoints` and the repair (`wear::field_amount_for`,
`bench_kit`); the recording member the runs (`blueprint::runs_for`); the
starter a run's seconds and unattended share, kept in `Job.pace` so a switch
or a save leaves a run as begun; the bearer the shot's cost. `drive_coil` and
`hull_plate` and eight schematics are taught by Fabricator Materials 1 to 3
and Weaponsmith frames.1, fittings.3, .4. A works foreman (`Prop::Foreman`, -5
spacings, `--at foreman`) teaches both trees.

Harvesters (ticket 0009, step 6): `assets/resources/harvest.ron` holds the
dials; the item `veyra:station.field_harvester` (kind `Harvester`) has a
schematic (Fabricator Materials 4) and sells at Pelagic's board. `Station {
field, op, value }` (`count`, `rate`, `hopper`) is read by `station_field`; no
Stations box is count 0. `strata_sim::harvest`: `Harvesters` (each keeps the
figures it was planted with), `can_plant`, `station`,
`Plant`/`Collect`/`PickUp`/`Lead` in `HarvestInbox`, `advance` on the
factory's `nominal_ticks`. The game stands props (`harvesters.rs`) and saves
them in the household file, stamped with the world; the client has
`ui::harvester` (F). Stills: `--harvester`, `--harvest-hours`.

The first hour (ticket 0009, step 7): `assets/balance/first_hour.ron` is the
path a new household walks, the assumptions no rule says and the minute bands;
`tools/balance/src/field.rs` is the model over the sim's own functions
(`just balance field`, `--write <md>`, `--questions`) and the `first_hour` test
of `tools/balance` holds it. `docs/balance/first-hour.md` says it in minutes,
with every dial moved and what a Surveyor box and a made carbine are worth.
Rerun it after a change to survey, harvest, session, price, XP or deposit data.

Guns with a role (ticket 0010, step 1): a `weapons.ron` row (version 5) has
`mode` (Single, Burst, Auto), `hold`, `bloom`, `move_spread`, `range` and
`falloff`; `bolt_range` is gone. `Combatant::step` is the scheduler (press
edge latched for the actor's `buffer`, owed burst rounds, the hold);
`cone_when` is the row's end plus bloom plus move share, which the reticle and
the bolt (`shot_cone`) read, and it supersedes the pistol cones above.
`Falloff::share_at` scales a bolt's damage at contact, `WeaponSpec::facts`
words the card, the sentry's `mode_override` is one bolt a charge. Range
presets `*-vs-dummy-{10,30,60}m` and `carbine-{hold,burst}-vs-dummy-8m` carry
five bands; `--hold-fire <s>` holds the trigger from frame 120.

Melee as a weapon (ticket 0010, step 2): `Weapon` has Blade, Greatblade and
Glaive beside the guns; `assets/combat/melee.ron` (in `CombatTables`' version)
scales the Light, Followup and Heavy rows per class after the sheet and
before the fitted row, a Blade exactly as before, and `swing_posture` shares a
hit taken mid cut. A `Combatant` carries two slots, any weapon in either, and
`CombatIntent.weapon` is the slot to draw (1 and 2). With a gun in hand a cut
is refused (`Refusal::NoBlade`); with a melee weapon the trigger's edge is the
light cut, Q the heavy. `Hands` refuse two of a family (`SameFamily`). The
items `veyra:weapon.{service_blade,yard_greatblade,pier_glaive}` sell at the
board; `--in-hand <item>[,<item>]` is for stills.
The guard (step 2b): aim held with a melee weapon in a Ready hand
(`Combatant::guard_up`). A hit is a `Blow` (direction, `BlowKind`); the pure
`guard_outcome` gives Open, Blocked, Parried or Broken from `melee.ron`'s
`guard` rows, `parry_rest` and `guard_regen`; a parried cut puts its riposte
on the swinger. Harness: `GuardAndCut` and the `guard-`, `a-parry-` bands;
`--hold-guard <s>` holds it for stills.
Melee made (ticket 0010, step 3): the three melee items stand on chassis of
`veyra-weapons` (`field_blade`, `heavy_cutter`, the new Morrow `reach_glaive`)
with mounts, baked pictures and models, and seven made parts
(`veyra:module.*`, taught by the Weaponsmith's Edges 1 to 4). `RecoveryTime`
is live for a melee weapon: a factor on the tail after `contact_end`, one
function `cut_clip_time` for the sweep and the pose. Liveness is asked of the
weapon (`fitting::live_on`, `live_for`: Capacity not on a blade, RecoveryTime
not on a gun). `content --weapons-only` bakes pictures and models alone;
`--learn <box>` teaches a still's member; `--fitted` stands a missing part.

The rifle (ticket 0010, step 4): a `Rifle` row in `weapons.ron` (version 6),
and every row gains `aim_time` (the cone closes over it, nought steps as
before) and `zoom`; `Combatant::aim_share` and `cone` read the closing, the
client's `scope` enters a scoped view at a closed aim on a zoom over 1.5.
`veyra:weapon.marksman_rifle` has four parts, five schematics (the Weaponsmith
teaches them); Ranger's Long shots 1 to 4 certify and improve the rifle, the
survey trainer teaches Ranger, `rates.wilderness` pays rifle damage and long
hits; `rifle-*-vs-dummy` presets, four bands, `--hold-aim`, `--orbit-yaw`.

Abilities (ticket 0010, step 5): `assets/combat/abilities.ron` (a table of
`CombatTables`) is two statuses (Focused: the aimed cone at its tightest;
Fortified: no posture damage taken and half of every blow) and a row an ability:
slot, works_with, stamina, cooldown, windup, length, cancel, pace and an effect (Cut, Shot,
Strike, Own). A box has `Grant` and `Ability` effects. The member's `Bar`
(`strata_sim::ability`, beside the sheet, saved with the member) is held by
`can_slot` and `BarRequest` in `BarInbox`; a press is `CombatIntent.ability`,
asked of `can_use` (`AbilityRefusal`), and runs as `Action::Ability`. Harness:
`--abilities`, `--b-abilities`, `PressAbilities`. Game: `--grant`, `--press`.
Abilities put right (step 9, third part): a status row may carry `damage_taken`
(Fortified, 0.5, read once in `Combatant::take` after the guard); focus is
instant; the presser presses a status when it can help (focused for a loose
shot, fortified before a cut in reach), a melee hand waits for a raised
guard to drop and keeps the stamina of its next cut, and an instant press
leaves the pull of its tick standing; the butt strike breaks a full posture;
the worth page's Abilities table reads each on its own fight.
Second half: four statuses (Slowed, Shield broken, Disarmed, Reflecting);
a Shot's `on_hit`, `jump`, `blast`, `breach` and `ShotCone::Hip`; a Strike's
`scaled`; `Hit`, `Move`, `armed`. Each is read once: a status in
`Combatant::hit` after the guard, jump and blast in `ability::splash`,
Reflecting where a guarded bolt is despawned, Slowed in `speed_scale`,
Disarmed in `begin` and `can_use`. Bands: `FirstCut`; closers are unchosen.
Melee reaches a gun (ticket 0010, step 5c): `melee.ron`'s `caught` row
multiplies a melee cut's posture and damage on a gun bearer it meets with no
guard, and the stagger when that cut breaks them (`Combatant::is_caught_by`,
in `hit`; the rusher's too, a butt strike never); a `Move`'s `cover` blocks
bolts from ahead while it runs; an `Own` ability of length 0 is instant
(reflect shot); `sprint` on the cutting policies. Chosen bands: a blade wins
in reach and from a 6 m thrust, open ground is the gun's; `melee-can-close`
(25 m against a player's carbine) is unmet and waits on step 8's opponents.
The bar and the loadout (ticket 0010, step 6): `strata_registry::icons`
reads `assets/icons/catalogue.json` (`IconCatalogue::file`, serde_json; a test
holds every exported path and every icon `abilities.ron` names).
`strata_client::ui::icons` names the weapon symbols and loads handles.
`action_bar::bar_view` maps `Combatant::can_use` to the `strata:bar` layer's
squares, `BarFeedback` flashes a refusal and a taken press; `status_view`
draws statuses over the vitals and under a hostile's plate. The sheet's
Loadout tab (`ui::sheet::loadout`) sends `BarRequest` and words the sim's
answer; a granting box shows its square. `--open-loadout` is for stills.
The trees teach (ticket 0010, step 7): a box's `Guard { field, op, value }`
(share, arc, stamina, posture, parry, riposte) is folded into
`EffectiveTables.guards`, and `Combatant::guard_row` is the one reader of a
class's guard; an unfitted actor reads the table. Blade's branches are One
hand, Guard, Greatblade and Glaive over the old ids (`blade.cuts`,
`blade.guard`, `blade.breaking`, `blade.footwork`); the heavy cut is the
novice's. A shipped-files test holds that every ability is granted by a box.

Combat worth (ticket 0010, step 8): `just balance worth [--bouts n] [--write
docs/balance/combat-worth.md]` writes the page of what each weapon, ability and
opponent is worth from the harness alone; the dials the pass moved are
`assets/balance/combat_tuning.ron`, and the `worth` test of `tools/balance`
holds its shape. Rerun it after a change to any combat table. A cutting policy draws its
arrival, its pauses, its step and its chain lag from the bout's seed, and the
opponents draw nothing.

Opponents (ticket 0010, step 8): `OpponentKind` picks `OpponentBrain`'s arm,
and `Seen` is what it sees of the target. The marksman holds 40 to 80 m, its
charge follows the aim until `lock` seconds before the shot
(`MarksmanTable::locked`), and `strata_client::combat_beam` draws that as a
beam. The duelist guards, is open while it swings, and lowers its guard once
to meet a Heavy it sees begin. Harness: `Policy::{Marksman, Duelist, Close}`.
Game: `--hostiles <kind>[@metres][,...]`.
The bulwark (step 9, second part): `OpponentKind::Bulwark` walks in at
`advance_speed` behind a raised guard and cuts a light and its follow up;
`GuardRow.bolts` (`BoltGuard`) is the one figure set a guard reads against a
bolt, set by no class row, only by `Combatant::with_guard` for the bulwark. A
blast is met by no guard (`Blow::blast`). Its first attack counts from its
arrival in reach, a cut its guard met holds its own back for `pinned`
(`Combatant::guard_met_for`; a bolt holds nothing) and a broken guard stays
down for `stagger_cooldown`. Harness: `Policy::{Bulwark, Punish}`.
`--hostiles bulwark[@m]` (the default fight has one); the client's slab is
`strata_client::combat_shield` (marker `Shield`).

Polish (ticket 0011, step 1): a weapon on the cursor can be taken
(`equip::find_for_take`, the old one leaving for the cursor) and a take
while the `Combatant` is not Ready is `EquipRefusal::MidAction`; T on a
pack weapon (pack-only screen) and a click on a hand with one carried push
the take. Every screen's root has `min_height` equal to `max_height`, with
scrolling regions inside, so a card or list moves nothing outside itself.
L opens the Loadout; `strata_sim::ability::unlearned` feeds its NOT LEARNED
rows, the bar draws `[+]` and "L: abilities" for a place that could be
filled, the crafting list has Known, Ready and All chips (`--crafting-filter`)
and six groups. Icons (step 2): `IconSpec.symbol` and `ClassDef.symbol` name
a library icon; `strata_registry::item_image` is the one rule (baked
picture, else symbol, else the stand-in shape; a painted square is never
tinted) and every drawer asks it. The bike's icon is baked by `content
--vehicles-only` (`VehicleModels.icons`); `--give <item>,..` fills a
still's pack.
Map (step 3): `strata_sim::chart` (`Chart::square`, `ground`, `site_marks`,
`marks`; north `+z` up, east `-x` right, as `Compass`) and the client's
`ui::map` on M: one picture a scale (3, 12, 48 km), made on the task pool
and kept, with the marks laid over it. Only found deposits are marked (and
the tracked one); `--open-map near|region|far` is for stills.
Sound and effects (step 4): the field's sounds are arms of the one `Sound`
table, hung on the sim's own events and answers (`combat_audio/field.rs`);
`LoopEmitter(EmitterSound)` is the looping emitter a game prop carries while
it runs. `Pulse` gained `Shape::Arc` and an optional mesh (a ping's standing
band, a cut's arc); status glows live as long as the status; `Feedback.trauma`
shakes the camera unless the Display tab's Camera shake is off; `PULSE_CAP`
(192) drops the oldest transient pulses. A kneeler (`Kneel`) is left by the IK.

Factions (ticket 0012, step 1): kinds, anchors and rules are
`assets/factions/` (`strata_registry::faction`);
`strata_worldgen::factions::FactionMap` makes a world's factions per land
mass from the seed and the sites (`holder_of`, `territory`; in no version,
no handshake; Port Serein is held by Pelagic Works); `strata_sim::factions`
has `Factions` and `HeldHere`; the place card names whose ground and who
holds a site, the map has a Holders chip, `just worldmap` a `-factions`
layer (`--no-factions`). An anchor holds its core (`core_m`), a faction's name
is rooted in its seat's place, and the outlaw camp is a site kind
(`strata:wild_camp`, rank 6, held by outlaw bands, group `veyra:outlaw_camp`, `--at camp`).

Encounters (ticket 0012, step 2): `assets/encounters`
(`strata_registry::encounter`) and `assets/combat/tiers.ron` (`tier_at`,
`opponent_pools`) are read by `strata_sim::encounters`: `plan`,
`NearEncounters`, `Allegiance`, `PlayedClock`, `Cleared` (saved in the
household), `Fights` (engagement per encounter; F5 lets a camp go). The game's
`camps.rs` shares `spawn_hostile` with the staging fight; the harness mixes
sides (`camp-t1..3-vs-carbine`); `--camp-tier`, `--cleared`.

Claims and deeds (ticket 0012, step 3): `assets/factions/claims.ron` and
`deeds.ron` (`strata_registry::faction`); `Deposit::grade_pm` and
`FactionMap::claim_of` (the top fifth by grade in held ground is a concession,
commons or reserve by the holder's nature). `strata_sim::deeds`: `Deeds` (a
hidden ledger, never shown), `score`, `stance`, `board_terms`, `Leases` (bought
at an anchor's board for played hours); the stance sets prices, shown only as
words. `--stance <faction>=<stance>` and `--lease` are for stills.

Caches and the fall (ticket 0012, step 4): `assets/loot` and `strata_sim::loot`
(`roll`, `Caches`, `CacheRequest`) leave a cache at a cleared camp; F empties
it into the pack. `assets/household/fall.ron` and `strata_sim::fall` carry a
member who falls in the world home, hurt (`Character::injured_until`) while the
next able member goes out. `--fall`, `--injured <member>` and `--cache`. Postings (step 4b):
`assets/procurement/postings.ron`, `strata_sim::postings` (`offered`, `Jobs`, `Known`),
`TakeJob` and `Collect`; the map shows a camp only heard of or found; `--take-job`,
`--job-done`, `--found`.
Posts (step 3b): `Allegiance::Faction`, `EncounterId::Post`, the neutral team
and `encounters::enemies` as the one enemy rule; working a claim without leave
starts a `Warning` that crosses the post when it runs out, as does a blow or a
hostile stance; Hurt and Killed deeds; a guard's F sells a lease; `--at claim`.
Wreck and machine (step 5a): site kinds `strata:wreck` (rank 7, group `veyra:wreck`) and
`strata:wreck_works` (rank 8, group `veyra:derelict_machine`), held but never a seat; `--at wreck`, `--at machine`.
People and salvage there (step 5b): `EncounterDef.crew` (`Holder`, `Unaligned`: no band, `Outlaw(None)`), scavengers at wrecks and
machine security at machines, `assets/loot/{wreck,machine}.loot.ron`, the board's WORK per kind (`Posting.kind`).

The name (ticket 0013, step 1): the engine crates are `strata-*` and the data
ids `strata:` (both were `scrolls`). Every seed and world hash salted with an
id reads it through `strata_core::stable_label`, which spells it `scrolls:`,
so no world moved and `world_version` is unchanged; the hash salts keep the
old word too. A household file under the old ids is read renamed and kept as
`.ids.bak` (`OLD_DATA_NAMESPACE` in `apps/game/src/household/save.rs`).

Native and web (ticket 0013): `strata_core::platform` has `par` (the one
parallel map) and `time` (`Instant`); data is read through the installed
`AssetSource` (`strata_registry::files`), saves through `strata_sim::store`;
apps/game has default `net` and empty `web` features. The wasm32 check:
`cargo check -p game --target wasm32-unknown-unknown --no-default-features --features web`
`cargo run -p xtask -- bundle <out.gz> [--raw f]` packs the readable data; `game --data-bundle <raw>`
reads it natively and `strata_sim::store::LocalStorage` (wasm) keeps saves under `veyra/*.ron`.
The page's query string is the command line (`?fresh&at=city`). `just web` builds `dist/`
(about 10 min); serve it with `python3 -m http.server` and open Chrome under `xvfb-run`.

## Commands

```
just            cargo check --workspace --all-targets
just test       cargo test --workspace
just lint       clippy, warnings fatal
just fmt        cargo fmt --all
just fmt-check
just doc        rustdoc, broken links fatal
just run        cargo run -p game -- <args>
just run-headless
just run-server cargo run --release -p server -- <args>     the dedicated server on UDP 5000
just run-client cargo run --release -p game -- --connect 127.0.0.1:5000
just worldmap   cargo run -p worldmap -- <args>
just server-check
just content    bake generated assets into generated/ (the game's `generated://` source)
just bench      the performance gates over the fixtures (release, GPU); `--record "<label>"` appends to docs/perf/ledger.md;
                `--xvfb` runs on a virtual display and gates on work time (main, render CPU and GPU), not the wall clock
                moving fixtures hold at their start until the world has loaded, then move; a walker that stops fails the run
just look       the four look fixtures captured to scratch/look/ with GPU cost in docs/perf/look-ledger.md: `--label "<text>" [--render-look p]`, `--compare <a> <b>`
just review     reference galleries of every building, and the weapon pictures' contact sheets, into scratch/review/
just balance    the duel harness (release): `just balance duel --matchup <preset> --vary <path>=v1,v2`, `just balance list`
just balance-check  the feel contracts of assets/combat/contracts.ron, release; the `contracts` test gates the same bands in `just test`
just ci         fmt-check lint test server-check
```

`cargo run -p game -- --frames 60 --screenshot shot.png` renders sixty
frames, saves the last one and exits, for checking a change visually.
The default is the character with an orbit camera; `--auto-walk` walks it
forward along `--look` without input, `--flycam` puts the free camera
back with no player, `--open-pause` pushes the pause menu on frame 120
for a screenshot of the screens. `--headless --frames 10` runs the
simulation with no window, with a scripted walker at the spawn point so
regions and colliders stream as they would on a server. `--connect
host:port` joins a running `server` (same `--seed`); the server spawns
the player and the client predicts it. `--release -- --flight
--flight-heading 1.5708 --spawn 15000,-12500 --screenshot "path-{frame}.png"
--shot-frames 300,3000` flies the scripted 200 m/s path for a minute,
logs fps and worst frame every two seconds and saves a screenshot at each
listed frame; `--spawn x,z` starts elsewhere (the default is dry land
near the origin), `--at city` or `--at road` at Port Serein's street or the
far end of its road (the server takes `--at` too), `--at deposit` or
`--at deposit:<type>` at the survey post of the nearest deposit to the
default spawn, `--at bench` before the city's workbench and fabricator, `--at board` before its procurement board
and `--at terminal` before its terminal,
`--look yaw` sets the initial heading, `--time F` the
time of day (0.5 noon), `--ev100`, `--haze` and `--no-shadows` tune the
sky, `--fog F` scales the valley fog (0 draws none and registers no
pass), `--weather fog,sea_fog` pins its weather (default `1,0.5`) and
`--fog-steps N` its raymarch steps, `--no-foliage` and `--wind F`
control the grass, `--no-rivers` builds no river or lake water, `--aa msaa4|off|taa` picks the anti-aliasing (the
valley fog needs `taa` or `off`), and `--window WxH` sizes the window
(default 2560x1440). In the
window Escape opens the pause menu (the sim keeps running under it), F1
the controls card (shown for eight seconds after a spawn or a swap), F3
toggles the debug statistics, off by default; `--stats` starts with them
on, F4 wireframe (the run must start with `--wireframe`), `[` and `]` scrub the time of
day. `--script-progression --script-hurt n` has the nearest hostile land
`n` table-valued blows on the player from four seconds into the fight
(1 for the hurt screenshot, 8 for the fall). The UI scales with the
window's height (`ui/scale.rs`, 1.25 at 1080p, 2 at 4K; the Display
tab pins it), so anything that writes a projected point into a node
divides by it through `combat_hud::ui_point`. Run screenshot checks under `xvfb-run -a -s "-screen 0 2560x1440x24"`:
the window stays off the desktop and Vulkan still renders on the GPU,
though presenting is slower, so its frame times are not a benchmark. A
`--flycam` with `--spawn` starts exactly there, over water too.
`cargo run -p xtask -- shrink in.png out.png --width 1024` scales a
screenshot for the docs. Shaders under `assets/shaders/` are loaded from
disk, so a shader edit needs no rebuild. An end-to-end check is a
`server` in the background and two `game --connect --auto-walk` clients
with `--screenshot`; kill the server with `pkill -f '^\./target/release/server'`
afterwards (anchored: the unanchored pattern also matches the shell whose
command line mentions the path).

## Conventions

- Bevy 0.19.1, edition 2024. Verify Bevy signatures against the source in
  `~/.cargo/registry`, not from memory: `On<E>` observers,
  `Message`/`MessageWriter` for buffered events, required components,
  `bevy_camera::Camera3d`, `Mesh3d`, `MeshMaterial3d`. The same goes for
  avian3d 0.7, bevy_replicon 0.44 and bevy_replicon_renet 0.20.
- Third-party crates go in `[workspace.dependencies]` with
  `default-features = false` and the features named by the member that
  needs them; append, never reorder.
- The replication protocol (`strata-net-replicon/src/protocol.rs`) is
  registered in one function in one order; add to the end, never in the
  middle, or every older client is refused.
- Client-side input systems that steer anything are gated with
  `.run_if(strata_client::ui::game_input_allowed)`; a screen on the
  slotted stack owns the keys and the mouse while it is up.
- Workspace lints: clippy pedantic warn, `unsafe_code` deny, `missing_docs`
  warn. Allow a pedantic lint locally with a one-line reason, not globally.
- Doc comments explain why, not what. No em-dashes in prose or comments.
- No combat literal in code: a tunable goes in `assets/combat/*.ron` and is
  read through `strata_sim::combat_tables`; the reproduction test in
  `combat_tables::tests` is what a table edit must update, and a changed
  number is a changed handshake version.
- `slotted-registry`, `slotted` and `slotted-test` come from the pinned
  git rev in `Cargo.toml`; `.cargo/config.toml` patches them to the
  sibling `bevy-slotted` checkout (`../../bevy-crates/bevy-slotted`). The
  ashlar crates are plain path dependencies on the sibling `ashlar`
  checkout (`../../bevy-crates/ashlar`) until that repository is pushed,
  after which they become git dependencies patched the same way; a change
  to a building crate is therefore visible here with no push and no rev
  bump, and needs ashlar's own `just ci` to pass. The
  glass theme and its fonts are copied into `assets/themes/` and
  `assets/fonts/` from that workspace. While that patch is in effect every
  cargo command rewrites `Cargo.lock` to the path sources, so the lockfile
  on a dev machine never matches a clean clone; CI drops the patch and
  does not build `--locked`.
- `strata-worldgen` uses only correctly rounded float operations per
  sample (no platform `sin`/`cos`/`hypot`/`powf`); the one transcendental
  goes through the `libm` crate so the world is the same bits on every OS.
- A SIGSEGV in a test binary after all its tests printed ok (debug build,
  many threads busy) is this machine's CPU (an i9-13900KF) failing under
  heavy multi-thread load. Rerun it; do not trace it, change stack sizes or
  add locks. Pools in tests and tools are capped at 16 threads
  (`strata_worldgen::measure::MAX_THREADS`, each test crate's
  `tests/common`, `worldmap --threads`) and a test binary runs one pool at
  a time. A wrong value from a test is a different matter and is a bug.
- Run `just ci` before finishing.
