# Veyra weapon workbench

A standalone Rust/Bevy assembly studio with 12 Ashlar weapon chassis,
41 compatible replacement components and 24 named configurations. Select a
chassis, fit its components, inspect the procedural meshes and materials, and
save or export the assembly. Combat tables in the game remain unchanged.

Run from the workspace root:

```sh
cargo run -p weapon-workbench
```

The studio opens on Vigil's patrol carbine with the Vigil catalogue selected.
**All** shows every maker. The four military chassis are `combat_pistol`,
`patrol_carbine`, `marksman_rifle` and `breacher`. The original chassis remain
`service_pistol`, `burst_rifle`, `precision_rifle`, `scattergun`,
`support_projector`, `field_blade`, `heavy_cutter` and `impact_baton`.

Start with a named configuration, or select a chassis and compatible components:

```sh
cargo run -p weapon-workbench -- --preset vigil_recon --finish field --view side
cargo run -p weapon-workbench -- --weapon burst_rifle \
  --module compact_optic --module rifle_extended_cell \
  --module rifle_cooled_fore_end
```

`--preset` chooses its chassis automatically and cannot be combined with
`--weapon` or `--config`. Repeated `--module` arguments apply after the initial
configuration. `--finish issued|field|bare` overrides its finish.

The workspace expects the local Ashlar and slotted checkouts configured in its
Cargo manifests and `.cargo/config.toml`. Interactive mode uses a window;
offscreen captures still require a working graphics adapter or software renderer.
No extra Cargo feature flags are required.

## Inspect and assemble

- Filter by maker on the left. Each side panel scrolls independently under the
  pointer, including the full chassis list and longer component lists.
- Choose Stock or either named configuration near the top of the fitting panel,
  then select a finish and individual components. Choices respect chassis slots.
- Drag either mouse button over the model to orbit; scroll there to zoom.
- **Side**, **Top** and **Perspective** select inspection views. Startup accepts
  `--view studio|side|top`.
- **Stock reference** toggles stock geometry in the same view while retaining the
  fitted selection. `--compare` starts this mode. Save and Export still use the
  fitted assembly; fitting another component or configuration exits comparison.
- Press **E** to separate components, **Space** to toggle rotation, and **R** to
  reset the view. Clay view reveals silhouette and surface geometry.
- **Stock build** restores stock components and the Issued finish.

Vigil adds dark coatings, lean receivers, restrained stocks and recessed field
fittings. The overall direction is moderately stylized: recognizable proportions,
readable surfaces and fine manufactured detail without exaggerated bulk.

Geometry comes from `crates/veyra-weapons`. Three self-contained Ashlar material
graphs provide fine coatings, machined metal and polymer. Shell, accent, steel
and grip use procedural bakes; ceramic, lens and emissive inserts use material
constants. Module factors remain design proposals, not measured combat balance.

## Save, load and export

**Save** writes `scratch/weapon-workbench/assembly.ron` by default. **Load** reads
that file and validates compatibility before applying it. `--output DIR` changes
the save and export directory. `--config FILE` loads an existing assembly at
startup and makes it the Save/Load destination. Original assembly variants and
module IDs remain supported.

**Export** writes these files into `--output`:

| File | Contents |
| --- | --- |
| `assembly.ron` | Fitted configuration, immediately reloadable with `--config` |
| `assembly.obj` | Assembled meshes with positions, normals, UVs and slot-based object names |
| `assembly.mtl` | PBR material constants and texture references |
| `weapon_*-*.png` | 24 maps across four textured slots: color, normal, packed ORM, roughness, metallic and occlusion |
| `assembly.recipe.ron` | Reusable Ashlar recipe with parts and attachments |
| `assembly.materials.ron` | Material definitions and graph parameters |
| `assembly.graphs.ron` | The three complete procedural material graphs |

The OBJ uses metres, Y up and +Z forward. Export retains the fitted, assembled
pose during stock comparison, exploded view or clay view. UVs include material
tiling. MTL uses the PBR extensions `Pr`, `Pm`, `norm`, `map_Pr` and `map_Pm`;
support varies between importers. Base color is sRGB; normal and ORM maps are
linear data. Export overwrites these named files in the selected directory.

Emit the current authoring catalogue without starting a renderer:

```sh
cargo run -p weapon-workbench -- \
  --catalogue docs/design/equipment/data/workbench-catalogue.ron
```

This typed RON export comes from the same Rust definitions used by the UI and
contains makers, chassis, slots, components, proposed effects and named presets.

## Offscreen capture and interaction exercise

Render a PNG and exit without opening a window:

```sh
cargo run -p weapon-workbench -- --preset vigil_recon --finish field \
  --capture scratch/weapon-workbench/recon.png
```

Add `--exploded` for separated components. `--width` and `--height` control the
window or capture size; defaults are 1440 by 960 and the minimum is 1120 by 800.

Generate 52 captures and an HTML index: stock and fitted versions of all
12 chassis, all 24 named presets, two exploded assemblies and two finish studies:

```sh
cargo run -p weapon-workbench -- --gallery scratch/weapon-workbench/gallery
```

Exercise native UI button actions before a capture:

```sh
cargo run -p weapon-workbench -- \
  --output scratch/weapon-workbench/exercise \
  --capture scratch/weapon-workbench/exercise/result.png --exercise
```

The 12-action exercise selects Vigil and its patrol carbine, fits Recon,
changes to Field finish, toggles stock comparison twice, chooses Side view,
explodes, saves, resets, reloads and exports. It checks that the fitted selection
was restored before capture. Use a dedicated output directory because the
exercise writes assembly and export files. `--exercise` requires `--capture`
and cannot be combined with `--gallery`.

Run `cargo test -p veyra-weapons -p weapon-workbench` for domain, geometry,
material graph and retained UI checks. Domain tests enumerate all 802 legal
configurations, round-trip every finish and bound composed proposal factors.
Geometry tests mesh stock chassis and compatible replacements through Ashlar's
manifold backend. See the [review record](../../docs/design/equipment/review.md)
for completed verification and remaining gameplay and animation work.
