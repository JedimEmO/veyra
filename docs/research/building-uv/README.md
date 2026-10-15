# Generated-building texture scale correction

2026-09-13. Same corporate tower, camera and lighting:

- [Before](before.png)
- [After](after.png)

Three contributors were corrected:

1. Corporate square textures previously covered unequal physical widths and
   heights, up to 4.8 by 7.2 metres. Concrete now repeats every 2.4 by 2.4 metres,
   and metal every 1.2 by 1.2 metres. Material definitions still allow intentional
   rectangular tiles; the engine does not enforce a style-specific size.
2. Dominant-axis projection discarded part of the surface length on angled
   geometry. `strata_buildings::planar_uvs` now builds an orthonormal face frame.
   Both CSG output and foundations/ramps use it. UV distances equal surface
   distances, including bevel and cutter faces. Coplanar triangles share the
   frame and origin. Curved surfaces still have projection seams; this is not
   an atlas unwrap or triplanar blending.
3. The source concrete albedo contained strong vertical streaks. Its generator
   now uses isotropic mottling, retaining pores and rough concrete detail. This
   shared texture also updates the original outpost. Normal and ORM maps are
   unchanged. Local runoff/weathering can be authored separately later.

Full `just ci` passed (formatting, Clippy, workspace tests and isolated headless
checks). The final tower preview was rendered and inspected.

The regression test measures all three UV edge lengths against physical lengths
on a rotated, bevelled box with a cutout. A pure test checks continuity across a
sloped quad's diagonal and rejects invalid triangles. Existing preview tests
check PBR tangents for both building kits.

Reproduce the final image after building `building-preview`:

```sh
RUST_MIN_STACK=8388608 taskset -c 0-3 nice -n 10 ./target/debug/building-preview \
  --serial-pipelines --scene corporate --zoom 0.9 --frames 60 \
  --screenshot docs/research/building-uv/after.png
```

The earlier corporate and terrain galleries remain historical captures from
before this correction.
