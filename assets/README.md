# assets

Game data and textures. Phase 1 adds `biomes/*.biome.ron` and terrain
materials here, Phase 5 the species in `flora/*.flora.ron` (what grows in
which biome, how dense, how steep, and a tree's generator parameters), and
ticket 0004's step 3 the geological provinces in `geology/*.geology.ron`
(what the ground is made of: its bare faces, how the biomes' relief takes
shape on it, its beds, how water cuts it and its rock kit); the directory
exists from Phase 0 so `AssetPlugin` has a root.

## `themes/`, `fonts/` and `icons/`

The UI theme, the two font families it names and the five hint icons the
slotted screens load, copied from the `bevy-slotted` workspace
(`assets/themes/glass.theme.ron`, `assets/fonts/manrope`,
`assets/fonts/barlow-condensed`, `assets/icons/hint_*.png`; the icons are
slotted's own and loaded by its item preview, so without them every
client run logs five missing-asset errors at startup). Slotted ships no
assets in its crates: a game copies the theme it wants into its own
`assets/`, which is what its examples do, and what `strata-client`
loads through the asset server at `themes/glass.theme.ron`. Both families
are under the SIL Open Font License 1.1; each directory carries the
`OFL.txt` it was published with, which is what redistribution asks for.
The theme file is the upstream one, unedited, so a later copy replaces it
whole; the roles a screen here needs (`hud.panel`, `hud.crosshair`, the
menu and control roles) are all in it.

## Buildings

Nothing here. Buildings and their materials are Rust (`veyra-buildings`,
`veyra-materials`), baked by `just content` into `generated/`, which the game
mounts as the `generated://` source. See
[the Rust building workflow](../docs/BUILDING_RECIPES.md).
