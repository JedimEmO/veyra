#!/usr/bin/env python3
"""Rasterize symbol masters with the system's librsvg and Cairo bindings."""

import json
from pathlib import Path
import sys

import cairo
import gi

gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg  # noqa: E402


def main():
    root = Path(__file__).resolve().parents[1]
    records = json.loads((root / "assets-src/icons/symbols.json").read_text())["icons"]
    destination = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "assets/icons/symbols"
    destination.mkdir(parents=True, exist_ok=True)
    for record in records:
        stem = record["stem"]
        source = root / "assets-src/icons/symbols" / f"{stem}.svg"
        handle = Rsvg.Handle.new_from_file(str(source))
        for size in (16, 24, 32, 64):
            surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
            context = cairo.Context(surface)
            viewport = Rsvg.Rectangle()
            viewport.x = viewport.y = 0
            viewport.width = viewport.height = size
            if not handle.render_document(context, viewport):
                raise RuntimeError(f"Failed to rasterize {source}")
            suffix = "" if size == 64 else f".{size}"
            surface.write_to_png(str(destination / f"{stem}{suffix}.png"))
    print(f"Rendered {len(records)} symbols at 16, 24, 32 and 64 pixels")


if __name__ == "__main__":
    main()
