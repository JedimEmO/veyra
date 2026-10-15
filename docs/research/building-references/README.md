# Fast building reference gallery

Run all generators, with no window or display server:

```sh
just building-references
```

Open `target/building-references/index.html`. The command builds incrementally,
then runs one GPU renderer for the whole batch. It writes 24 PNGs: three fixed
views for each of `basic`, `facade`, `entrance`, `outpost`, `office`, `corporate`,
`corporate-annex` and `corporate-block`. New preview scene enum variants are
included automatically. The HTML gallery labels every scene and angle;
`manifest.tsv` records cameras, dimensions and geometry counts.

For a single style:

```sh
just building-references --reference-scenes corporate
```

For a custom directory or a selection of styles, use the built binary directly:

```sh
./target/debug/building-preview --references /tmp/building-look \
  --reference-scenes outpost,corporate,corporate-block
```

Known filenames are overwritten. Other files in the output directory are left
alone; the gallery and manifest list only the current selection.

## Regression checks

Save a baseline in a separate directory, then render and compare:

```sh
./target/debug/building-preview --references /tmp/building-baseline
just building-references --reference-baseline /tmp/building-baseline
```

A changed or missing image returns a nonzero exit status. Changes are listed by
filename, and the generated gallery remains available for review. Comparison
uses decoded RGB pixels, not PNG encoding bytes. Baseline and output must be
different directories. Use the same ordered scene selection for baseline and
current run. A subset renders faster, but draw ordering at overlapping surface
edges can differ slightly from that style's image inside the full batch.

```sh
./target/debug/building-preview --references /tmp/corporate-baseline --reference-scenes corporate
just building-references --reference-scenes corporate --reference-baseline /tmp/corporate-baseline
```

Exact comparison is intended for the same GPU, driver and renderer version.
Treat differences after a driver/backend change as requiring visual review.
Geometry is automatically framed by its bounds, so changing its overall size
also changes camera distance; the manifest makes this visible.

## Fixed rendering contract

- 800×600, MSAA 4×, static studio lights and background shared with the preview.
- Front: yaw −0.5, elevation 0.25 radians. Rear: yaw π−0.5, same elevation.
  Elevated: yaw −0.65, elevation 0.65.
- Model centered on its bounds, camera distance 1.4 times its bounds diagonal.
- No world streaming, UI, window, live input or gamepad initialization.
- Offscreen image target, fixed 1/60-second simulation steps, 12 warmup frames
  after textures load and after each camera change.
- Serial shader compilation and direct drawing. Direct drawing avoids the small
  ordering differences observed at coincident surface edges with indirect draws.
- One render process, reused pipeline/material resources, four worker threads.
  The `just` command also applies four-core affinity and reduced process priority.
- Asset, save, baseline and 120-second timeout failures return nonzero.

This is headless GPU rendering, not the engine's renderer-free server mode. It
needs a supported graphics adapter but does not need X11, Wayland or Xvfb.

## Verification, 2026-09-13

Full `just ci` passed, including CLI mode tests, formatting, Clippy and the
workspace/headless checks. The final `just building-references` launcher was
verified with its baseline check and no display variables.

With `DISPLAY` and `WAYLAND_DISPLAY` removed, all 24 images rendered in 2.46–2.56
seconds on the local RTX 4090, excluding compilation and using a warm driver
cache. Two complete runs were pixel-identical; the baseline command also passed.
The single-style repeat check also passed (three images in 1.42 seconds), and
a deliberately changed baseline pixel correctly returned exit status 1.
All scenes/angles were inspected together. First-run shader compilation or a
slower GPU can increase the time.
