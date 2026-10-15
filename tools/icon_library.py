#!/usr/bin/env python3
"""Export icon sources and build a catalogue that also works from a file URL."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets-src/icons"
ASSETS = ROOT / "assets/icons"
DOCS = ROOT / "docs/design/icons"


def read(name):
    return json.loads((SOURCE / name).read_text())


def relative(path):
    return path.relative_to(ROOT).as_posix()


def asset_path(path):
    return path.relative_to(ROOT / "assets").as_posix()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def search_text(row, selection):
    """Index positive meanings; contrasts must not make an unrelated icon match."""
    return " ".join([
        row["id"], row["label"], row["group"], *row["tags"],
        selection["description"], selection["depicts"], selection["action"],
        selection["role"], selection["effect"], *selection["subjects"], *selection["palette"],
        *selection["use_cases"],
    ]).lower()


def manifest(allow_incomplete):
    review = read("review.json") if (SOURCE / "review.json").exists() else {}
    selections = read("selection.json")["icons"]
    entries = []
    for family, rows in (("symbol", read("symbols.json")["icons"]),
                         ("ability", read("ability-briefs.json")["icons"])):
        folder = "symbols" if family == "symbol" else "abilities"
        for row in rows:
            stem = row["stem"]
            extension = "svg" if family == "symbol" else "png"
            source = SOURCE / folder / f"{stem}.{extension}"
            if not source.exists():
                if allow_incomplete:
                    continue
                raise ValueError(f"Missing source: {relative(source)}")
            runtime = ASSETS / folder / f"{stem}.png"
            sizes = (16, 24, 32, 64) if family == "symbol" else (64, 256)
            exports = {
                str(size): asset_path(ASSETS / folder / f"{stem}{'' if size == sizes[-1] else '.' + str(size)}.png")
                for size in sizes
            }
            assessed = review.get(row["id"], {})
            source_hash = digest(source)
            if assessed.get("source_sha256") != source_hash:
                assessed = {}
            selection = selections[row["id"]]
            entries.append({
                "id": row["id"], "label": row["label"], "family": family,
                "group": row["group"], "tags": row["tags"], "meaning": row["meaning"],
                "selection": selection, "search_text": search_text(row, selection),
                "asset_path": asset_path(runtime), "source_path": relative(source),
                "exports": exports, "tintable": family == "symbol",
                "status": "ready" if assessed else "unreviewed",
                "minimum_reviewed_px": assessed.get("minimum_px"),
                "review_note": assessed.get("note", "Pending visual review"),
                "reference": "docs/design/icons/" + ("symbol-directions.png" if family == "symbol" else "ability-direction.png"),
                "provenance": "Authored SVG" if family == "symbol" else "Built-in image generation; full prompt in assets-src/icons/ability-briefs.json",
                "gameplay_binding": None,
                "source_sha256": source_hash, "export_sha256": digest(runtime),
                "export_hashes": {size: digest(ROOT / "assets" / path) for size, path in exports.items()},
            })
    return {"version": 2, "asset_root": "assets", "icons": entries}


def export_abilities(allow_incomplete):
    destination = ASSETS / "abilities"
    destination.mkdir(parents=True, exist_ok=True)
    for row in read("ability-briefs.json")["icons"]:
        source = SOURCE / "abilities" / f"{row['stem']}.png"
        if not source.exists():
            if allow_incomplete:
                continue
            raise ValueError(f"Missing source: {relative(source)}")
        with Image.open(source) as image:
            if image.width != image.height or image.width < 256:
                raise ValueError(f"Expected a square master of at least 256 pixels: {source}")
            if image.mode == "RGBA" and image.getchannel("A").getextrema() != (255, 255):
                raise ValueError(f"Ability artwork must have an opaque background: {source}")
            image = image.convert("RGB")
            for size in (64, 256):
                suffix = "" if size == 256 else ".64"
                image.resize((size, size), Image.Resampling.LANCZOS).save(
                    destination / f"{row['stem']}{suffix}.png", optimize=True)


def validate(data):
    ids = set()
    source_ids = {r["id"] for name in ("symbols.json", "ability-briefs.json") for r in read(name)["icons"]}
    if set(read("selection.json")["icons"]) != source_ids:
        raise ValueError("Selection metadata must cover every source ID exactly")
    for row in data["icons"]:
        if row["id"] in ids or not re.fullmatch(r"veyra:icon\.[a-z0-9.-]+", row["id"]):
            raise ValueError(f"Invalid or duplicate ID: {row['id']}")
        ids.add(row["id"])
        selection = row["selection"]
        for key in ("description", "depicts", "action", "distinction"):
            if not isinstance(selection.get(key), str) or not selection[key].strip():
                raise ValueError(f"Missing selection field {key}: {row['id']}")
        for key in ("subjects", "palette", "use_cases"):
            if not isinstance(selection.get(key), list) or not selection[key] or not all(isinstance(v, str) and v.strip() for v in selection[key]):
                raise ValueError(f"Missing selection list {key}: {row['id']}")
        if selection["role"] not in ("symbol", "activated", "passive", "status"):
            raise ValueError(f"Invalid role: {row['id']}")
        if selection["effect"] not in ("none", "beneficial", "harmful"):
            raise ValueError(f"Invalid effect: {row['id']}")
        if any(related not in source_ids or related == row["id"] for related in selection["related_ids"]):
            raise ValueError(f"Invalid related icon: {row['id']}")
        source = ROOT / row["source_path"]
        if digest(source) != row["source_sha256"]:
            raise ValueError(f"Stale source hash: {source}")
        if row["family"] == "symbol":
            tree = ET.parse(source)
            if tree.getroot().get("viewBox") != "0 0 32 32":
                raise ValueError(f"Unexpected symbol canvas: {source}")
        if row["status"] == "ready" and row["minimum_reviewed_px"] not in (16, 24, 32, 48, 64):
            raise ValueError(f"Missing reviewed size: {row['id']}")
        for size, path in row["exports"].items():
            file = ROOT / "assets" / path
            if digest(file) != row["export_hashes"][size]:
                raise ValueError(f"Stale export hash: {file}")
            with Image.open(file) as image:
                image.load()
                if image.size != (int(size), int(size)):
                    raise ValueError(f"Wrong export dimensions: {file}")
                rgba = image.convert("RGBA")
                alpha = rgba.getchannel("A")
                if row["tintable"]:
                    if alpha.getextrema() != (0, 255):
                        raise ValueError(f"Symbol must have opaque shapes and transparency: {file}")
                    if any(a and (r, g, b) != (255, 255, 255) for r, g, b, a in rgba.getdata()):
                        raise ValueError(f"Symbol must be white for tinting: {file}")
                elif alpha.getextrema() != (255, 255):
                    raise ValueError(f"Ability export must be opaque: {file}")
        if digest(ROOT / "assets" / row["asset_path"]) != row["export_sha256"]:
            raise ValueError(f"Stale export hash: {row['id']}")
    if not ids:
        raise ValueError("Empty catalogue")
    print(f"Validated {len(ids)} IDs, sources, hashes, export sizes, alpha and tint rules")


def font(size):
    return ImageFont.truetype(str(ROOT / "assets/fonts/manrope/Manrope-Variable.ttf"), size)


def contact_sheets(data):
    destination = DOCS / "review"
    destination.mkdir(exist_ok=True)
    for family in ("symbol", "ability"):
        rows = [row for row in data["icons"] if row["family"] == family]
        batch = 30 if family == "symbol" else 12
        columns = 5 if family == "symbol" else 4
        cell_w, cell_h = (230, 142) if family == "symbol" else (280, 235)
        for start in range(0, len(rows), batch):
            selected = rows[start:start + batch]
            height = 75 + ((len(selected) + columns - 1) // columns) * cell_h
            sheet = Image.new("RGB", (columns * cell_w + 32, height), "#141A24")
            draw = ImageDraw.Draw(sheet)
            draw.text((16, 12), f"Veyra {family} review / {start + 1} to {start + len(selected)}", font=font(22), fill="#E6EDF3")
            for i, row in enumerate(selected):
                x = 16 + i % columns * cell_w
                y = 65 + i // columns * cell_h
                sizes = (16, 24, 32, 64) if family == "symbol" else (32, 48, 64, 96)
                dx = x
                for size in sizes:
                    path = ROOT / "assets" / row["exports"].get(str(size), row["asset_path"])
                    with Image.open(path) as source:
                        thumb = source.convert("RGBA").resize((size, size), Image.Resampling.LANCZOS)
                    sheet.paste(thumb, (dx, y + 96 - size), thumb)
                    draw.text((dx, y + 101), str(size), font=font(10), fill="#91A4B7")
                    dx += size + 7
                draw.text((x, y + 121), row["label"], font=font(12), fill="#E6EDF3")
                if family == "ability":
                    # A second 64-pixel preview checks grayscale identity and a cooldown overlay.
                    with Image.open(ROOT / "assets" / row["asset_path"]) as source:
                        thumb = source.resize((64, 64), Image.Resampling.LANCZOS)
                        sheet.paste(thumb.convert("L").convert("RGB"), (x, y + 152))
                        sheet.paste(thumb, (x + 78, y + 152))
                        overlay = Image.new("RGBA", (64, 64), (0, 0, 0, 0))
                        ImageDraw.Draw(overlay).pieslice((-10, -10, 74, 74), -90, 90, fill=(0, 0, 0, 160))
                        sheet.paste(overlay, (x + 78, y + 152), overlay)
            sheet.save(destination / f"{family}-{start // batch + 1:02}.png", optimize=True)


def documentation(data):
    template = (ROOT / "tools/icon_gallery.html").read_text()
    payload = json.dumps(data, separators=(",", ":")).replace("</", "<\\/")
    (DOCS / "index.html").write_text(template.replace("__ICON_DATA__", payload))
    lines = ["# Icon catalogue", "", f"{len(data['icons'])} individual icons with source files and runtime exports.",
             "Open the [searchable gallery](index.html) to preview sizes and copy asset paths.",
             "The [JSON manifest](../../../assets/icons/catalogue.json) is the machine-readable index.", "",
             "Paths in `asset_path` are relative to Bevy's `assets/` root.",
             "Artwork IDs are stable lookup keys for this collection; they are not registered gameplay abilities.",
             "Painted art is untinted. Symbols are white with alpha and take the UI theme colour.", "",
             "Generated by `tools/icon_library.py` from the source metadata and visual review record.",
             "Edit those inputs, then rebuild this index.", ""]
    for family in ("symbol", "ability"):
        lines += ["## " + ("Solid symbols" if family == "symbol" else "Painted actions and effects"), "",
                  "| ID | Meaning | Runtime image | Smallest reviewed size | Status |", "|---|---|---|---|---|"]
        for row in data["icons"]:
            if row["family"] == family:
                path = "../../../assets/" + row["asset_path"]
                minimum = row["minimum_reviewed_px"]
                lines.append(f"| `{row['id']}` | {row['label']} | [PNG]({path}) | {str(minimum) + ' px' if minimum else 'Pending'} | {row['status']} |")
        lines.append("")
    lines += ["## Study IDs", "", "The six `veyra:icon.study.*` IDs in the original study remain reference-only.",
              "Production artwork has separate `veyra:icon.ability.*` IDs.",
              "No study ID has been reassigned to a gameplay ability.", ""]
    (DOCS / "catalogue.md").write_text("\n".join(lines))
    descriptions = ["# Painted icon index", "", "Searchable descriptions and selection notes for every painted icon.",
                    "Generated from `assets-src/icons/selection.json`. Use the [gallery](index.html) for visual comparison.",
                    "These are suggested artwork uses, not implemented gameplay definitions.", ""]
    for row in data["icons"]:
        if row["family"] != "ability":
            continue
        s = row["selection"]
        descriptions += [f"## {row['label']}", "", f"`{row['id']}`", "", s["description"], "",
                         f"Depicts: {s['depicts']}", "",
                         f"Role: {s['role']}. Subjects: {', '.join(s['subjects'])}. Colours: {', '.join(s['palette'])}.", "",
                         f"Suggested uses: {', '.join(s['use_cases'])}.", "", s["distinction"], "",
                         "Related: " + ", ".join(f"`{id}`" for id in s["related_ids"]) + ".", "",
                         f"[Runtime PNG](../../../assets/{row['asset_path']}) | Minimum reviewed size: {row['minimum_reviewed_px'] or 'pending'} px | {row['status']}", ""]
    (DOCS / "painted-index.md").write_text("\n".join(descriptions))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Validate the existing outputs without rewriting them")
    parser.add_argument("--find", help="Search positive meanings, subjects, visual descriptions, colours and tags")
    parser.add_argument("--family", choices=("symbol", "ability"), help="Filter search results by art family")
    parser.add_argument("--role", choices=("symbol", "activated", "passive", "status"), help="Filter by suggested UI role")
    parser.add_argument("--colour", help="Filter by one indexed palette name, such as blue or green")
    parser.add_argument("--size", type=int, help="Return ready icons reviewed for this display size or smaller")
    parser.add_argument("--allow-incomplete", action="store_true", help="Build an interim gallery while paintings are being authored")
    parser.add_argument("--svg-python", default="/usr/bin/python3", help="Python with gi, librsvg and cairo bindings")
    args = parser.parse_args()
    if args.find is not None or args.family or args.role or args.colour or args.size is not None:
        data = json.loads((ASSETS / "catalogue.json").read_text())
        terms = (args.find or "").lower().split()
        found = [row for row in data["icons"] if all(term in row["search_text"] for term in terms)
                 and (not args.family or row["family"] == args.family)
                 and (not args.role or row["selection"]["role"] == args.role)
                 and (not args.colour or args.colour.lower() in row["selection"]["palette"])
                 and (args.size is None or (row["status"] == "ready" and row["minimum_reviewed_px"] <= args.size))]
        print(json.dumps([{key: row[key] for key in (
            "id", "label", "family", "asset_path", "exports", "tintable", "status", "minimum_reviewed_px", "review_note", "selection")}
            for row in found], indent=2))
        return
    if args.check:
        data = json.loads((ASSETS / "catalogue.json").read_text())
        validate(data)
        expected = {r["id"] for file in ("symbols.json", "ability-briefs.json") for r in read(file)["icons"]}
        if expected != {r["id"] for r in data["icons"]}:
            raise ValueError("Manifest does not contain every source record")
        if data != manifest(False):
            raise ValueError("Manifest is stale; rebuild from current metadata and review records")
        if any(r["status"] != "ready" for r in data["icons"]):
            raise ValueError("Some assets have not completed visual review")
        return
    subprocess.run([args.svg_python, str(ROOT / "tools/icon_render_svg.py")], check=True)
    export_abilities(args.allow_incomplete)
    data = manifest(args.allow_incomplete)
    validate(data)
    (ASSETS / "catalogue.json").write_text(json.dumps(data, indent=2) + "\n")
    documentation(data)
    contact_sheets(data)
    print(f"Wrote {relative(DOCS / 'index.html')}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, FileNotFoundError) as error:
        sys.exit(str(error))
