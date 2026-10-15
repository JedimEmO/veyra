# Icon generation briefs

Use the approved sheet as an image reference when generating painted ability art.
The written recipe supplements the image; it does not replace visual comparison.

The 96 production paintings have their exact submitted prompts and reference
paths in [ability-briefs.json](../../../assets-src/icons/ability-briefs.json).
Each used the built-in image generation tool with `transparent_background: false`
and `ability-direction.png` attached as the style reference. Each output was
generated separately, then resized for runtime without compositing or repainting.
The shared wording is in [ability-style.txt](../../../assets-src/icons/ability-style.txt).
Inspect a neighbouring [source master](../../../assets-src/icons/abilities/)
as well as the approved sheet when extending the collection.

## One painted ability

Replace the bracketed fields with a real ability's meaning and visual brief.
Generate one square asset at a time so framing and export are unambiguous.
The background is painted and opaque.

```text
Use case: stylized-concept.
Asset: one square painted active-ability icon for Veyra, a grounded sci-fi frontier RPG.
Reference image: docs/design/icons/ability-direction.png, style only.
Match the large painted tiles' brushwork, broad forms, contrast, tight crop and detail budget.
Subject: [one physical subject and one specific action].
Composition: [dominant silhouette and movement direction].
Palette: [dominant hue, contrasting focal light, coloured shadows].
Readable silhouette, one focal action, broad confident brush shapes, dark corners.
Fill the square with the painting. Keep the main action readable at 32 to 64 pixels.
Use Veyra's practical equipment, tools, medical devices and industrial materials.
No text, numbers, labels, logos, borders, keybindings, cooldown masks or status badges.
No contact sheet, decorative circuitry, generic magic runes or photorealistic product rendering.
```

Keep a new asset's full prompt alongside its high-resolution source.
Record the reference image, tool and any later corrections.
Inspect the generated subject and anatomy, then review the exported sizes beside related abilities.

## Solid symbols

Author these as editable SVG shapes using column B of `symbol-directions.png`.
The reference controls silhouette weight, broad cutouts and simplicity.
Do not trace raster noise, lighting, accidental tiny details or the unapproved columns.
For expansion, copy the nearest production SVG's canvas and geometry conventions.

## Reference generation records

Both sheets used the built-in image generation tool with `transparent_background: false`
and no input images. The prompts below are the submitted text.
They explain the studies' provenance; use the single-asset recipe above for production.

### Symbol comparison

Output: [symbol-directions.png](symbol-directions.png). Column B was selected.

```text
Use case: ui-mockup. Create a precise art direction comparison sheet for a SCI-FI GAME SYMBOL LIBRARY, not item illustrations. Landscape board. Exact title 'Veyra / Interface symbols'. Exact subtitle 'Categories, skills and actions. Item previews come from their 3D models.' Dark navy #141A24 background and off-white #E6EDF3 icons. Absolutely flat VECTOR-LIKE graphics, crisp clean edges, NO textures, NO grunge, NO pictorial 3D objects, NO volumetric shading, NO rendered miniatures, NO glow. Three equal columns each depicting the SAME NINE SYMBOLS in the SAME 3 by 3 layout, with readable labels below each. Exact column headings: 'A / Instrument lines', 'B / Cut silhouettes', 'C / Technical duotone'. Nine symbols: Pistol (short barrel distinct handle and trigger), Carbine (long gun with stock and magazine), Blade (single utilitarian straight knife); Body armour (torso plate with neck opening), Carry pack (backpack with simple flap), Optic (scope side-profile with two mount feet); Footwork (boot plus one sweeping directional arc), Guard (single blade blocking an incoming diagonal stroke), Survey (probe crossing three ground strata). Each icon should be a simplified generic category symbol, not a particular item or manufacturer. Column A uses refined consistent medium-weight outline strokes, squared ends, gently chamfered corners, no decorative circuitry. Column B uses bold SOLID silhouettes with a few generous negative-space cuts, useful industrial sign pictograms, chamfered forms, distinctive clear silhouettes, no enclosing badges, no mandatory hexagons, black gaps separate functional parts. Column C uses the EXACT SAME solid silhouettes as B with one secondary muted blue-grey plane per icon adding controlled depth, maximum two flat colours, no gradients. All guns point right, all knives point up-right, boots point right. Keep equal optical weight rather than equal bounding boxes. Separate columns with generous space and quiet hairline dividers. At bottom: a small sample toolbar headed 'In use' showing ivory pack, search, filter, sort and close symbols, with the pack selected via pale ice-blue #7FD1FF underline and an amber #FFB454 repair wrench with a small warning triangle beside it. Show UI status outside the symbol artwork. Bottom text exactly 'One meaning per symbol. Colour belongs to state.' No other slogans, logos, branding or invented text. This is a professional readable icon style comparison intended to choose a durable extensible visual language for a grounded frontier MMO RPG. Be meticulous about consistent style within each column and meaningful differences between columns.
```

### Painted abilities

Output: [ability-direction.png](ability-direction.png). The painted treatment was approved.

```text
Use case: stylized-concept. Create a single polished concept sheet exploring PAINTED ACTIVE ABILITY ICONS for Veyra, a grounded sci-fi frontier MMO RPG. The user explicitly wants the visual language of classic World of Warcraft ability icons: bold hand-painted forms, saturated local colour, dramatic light-dark separation, tight dynamic crops, one unmistakable action per square, memorable at hotbar size. Translate this into ORIGINAL sci-fi industrial equipment and human actions. Not flat pictograms, not photorealistic miniatures, not item catalogue renders, not generic glossy mobile-game icons. Broad confident painted shapes, simplified anatomy, strong diagonals, rich coloured shadows, almost no fine surface detail. Every tile has a FULL-BLEED painted square background, with darker corners and bright focal action. Six equal large square icons, in a 3-column by 2-row grid on a quiet navy #141A24 design board. Heading exactly 'Veyra / Active abilities'. Small subtitle exactly 'Painted action studies'. Small readable labels OUTSIDE the squares below each. Icon 1 'Evasive step': a large cropped rugged sci-fi boot sweeping diagonally to the lower right, deep cobalt shadows and a curved electric blue motion stroke, athletic movement, foot silhouette fills most of square. Icon 2 'Parry': close-up of one industrial steel blade held by a work-worn armoured glove intercepting a second blade, one enormous diagonal SILVER blocking silhouette, concentrated AMBER impact spark at crossing, deep burgundy shadow background. Icon 3 'Aimed burst': extreme close-up side-three-quarter crop of compact futuristic carbine front and upper receiver firing a tightly grouped burst to upper right, brilliant ORANGE muzzle flash and three directional light streaks, burnt orange and charcoal palette, instantly reads as shooting rather than an object portrait. Icon 4 'Field repair': a large gloved hand pressing an industrial tool against a broken plate, bright YELLOW arc and chunky flying metal sparks, olive and ochre shadows, tool and hand fill square, no toolbox. Icon 5 'Survey pulse': a prominent hand-held geological scanner sending concentric TEAL scanning bands down across a single wedge of layered terrain, rich petrol blue shadows, an abstract purposeful equipment action, no tiny screen text or elaborate scenery. Icon 6 'Trauma patch': large gloved fingers applying a compact white medical injector patch to a forearm, concentrated GREEN light at the application point, dark emerald background, recognizable healing action through concrete medical equipment, no magic runes or fantasy potion. Consistent painterly craft and detail budget across all six, but each has its own dominant hue and silhouette. NO logos, lettering inside icons, numbers, cooldown masks, ornate frames, rarity colours on borders, skull decoration, holographic grids or excessive tiny particles. A narrow bottom row under heading 'Alongside category symbols' shows six small clean ivory BOLD SOLID FLAT industrial pictograms: pistol, carbine, blade, torso armour, backpack, optic. These small monochrome category icons are a distinct companion family to the colourful ability artwork; no outlines, generous negative space cuts. Footer exactly 'Concept abilities / not a gameplay specification'. Do not invent any other text. Landscape layout with comfortable margins, quiet neutral separators. The purpose is to approve a sci-fi interpretation of WoW-like painted ability icons alongside solid monochrome UI symbols.
```
