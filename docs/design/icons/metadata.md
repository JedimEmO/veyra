# Choosing an icon

The [JSON index](../../../assets/icons/catalogue.json) describes every shipped icon.
Use it to choose artwork by meaning and appearance, then inspect the actual PNG.
The [painted index](painted-index.md) presents the same selection notes as Markdown.

For a blue defensive action displayed at 48 pixels:

```sh
python3 tools/icon_library.py --find shield --family ability --role activated --colour blue --size 48
```

The result includes asset paths, size variants, review notes and the full `selection` record.
`--size` excludes unreviewed artwork and icons whose minimum reviewed size exceeds the requested size.
Omit it while exploring artwork that has not completed review.

Search is case-insensitive and requires every query word to appear somewhere in the positive description or tags.
It matches parts of words; it does not rank results or interpret natural-language requests.
Use short concrete terms such as `shield`, `healing`, `helmet` or `cable`.
Contrast notes are excluded from search, so mentioning an alternative does not create a false match.

## Selection fields

These fields live under `selection` on each manifest entry.
They describe artwork and possible UI uses, not implemented abilities or balance rules.

| Field | Meaning |
|---|---|
| `description` | A short statement of the action or condition represented. |
| `depicts` | Visible subject, composition and lighting, checked against the artwork. |
| `action` | A short action or condition label. |
| `role` | `activated`, `status`, `passive` or `symbol`; the intended presentation role. |
| `effect` | `beneficial`, `harmful` or `none`; distinguishes status meanings. |
| `subjects` | Concrete objects and body parts visible in the painting. |
| `palette` | Searchable colour names; descriptive, not gameplay element assignments. |
| `use_cases` | Suggested places to use the image, such as a hotbar or status display. |
| `distinction` | Why to choose this image instead of a closely related one. |
| `related_ids` | Stable artwork IDs to inspect when comparing alternatives. |

For example, Shield recharge shows a wrist emitter restoring personal protection.
Barrier dome shows a ground emitter projecting area protection.
Shield broken shows collapsed protection and has the `status` role with a `harmful` effect.
Their shared shield tags make them discoverable; their descriptions separate the meanings.

## Delivery fields

| Field | Meaning |
|---|---|
| `id` | Stable `veyra:icon.*` artwork ID. It is not a registered gameplay identifier. |
| `family` | `ability` for paintings or `symbol` for monochrome SVG-derived art. |
| `group`, `tags` | Subject grouping and positive search vocabulary. |
| `asset_path` | Default runtime PNG path, relative to `assets/`. |
| `exports` | Map from pixel size to runtime path, also relative to `assets/`. |
| `source_path` | Master artwork path, relative to the repository. |
| `tintable` | True for white symbols; false for paintings. |
| `status` | `ready` after visual review or `unreviewed` after a source change. |
| `minimum_reviewed_px` | Smallest recommended display size from the contact-sheet review. |
| `review_note` | Legibility limits and integration checks for this asset. |
| `source_sha256`, `export_hashes` | Content hashes used to detect stale artwork and exports. |
| `search_text` | Generated positive search text used by the gallery and command-line search. |

The manifest has `version: 2` and `asset_root: "assets"`.
Every icon keeps its previous stable ID and default runtime path.
The `meaning` field remains for compatibility; use `selection.description` for concise display text.

## Maintaining the index

Edit [selection.json](../../../assets-src/icons/selection.json) for descriptive metadata.
Add the exact same ID to the relevant symbol or painted source catalogue.
Keep generation instructions and exact prompts in `ability-briefs.json`;
selection descriptions should say what is visible in the delivered artwork.

The builder requires selection metadata for every source ID and validates related IDs.
Rebuild with `python3 tools/icon_library.py`, inspect the image and review sheet,
then update the visual review record and run `python3 tools/icon_library.py --check`.
The generated JSON, gallery and painted index all come from those same records.
