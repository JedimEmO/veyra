#!/usr/bin/env python3
"""Put the ranger's body into the value budget and give it a tint mask.

The body came from a generator (assets/characters/combat/frontier_ranger.bundle.json)
whose texture sits at a mean linear luminance of 0.02, so every character drew as a
black silhouette and the household colour multiplied black (review step six, F1
item 4). The generator is not rerunnable from this checkout, so this works on the
glb it made:

  1. the base colour texture is levelled: the blue-grey cloth patches (found by
     hue and value on a blurred copy) are lifted to a mean luminance of CLOTH,
     desaturated a little so a household or faction colour lands as that colour
     and not as a muddy blend, and everything else (gear, straps, trim) is lifted
     with a floor to GEAR, so the whole map is at the character family's band in
     assets/look/values.ron;
  2. the one triangle list is split in two primitives, `Cloth` and `Gear`, by
     where each triangle's centroid falls in the cloth mask, and the one material
     into two of those names, so `strata_client::combat_art` tints the cloth
     panels only. The vertex data, skin and animations are untouched.

    git show 30472be:assets/characters/combat/frontier_ranger.glb > in.glb
    python3 tools/character_values.py in.glb assets/characters/combat/frontier_ranger.glb

It writes frontier_ranger.values.json beside the output: what it measured and
did, so the change can be redone or judged.
"""
import json
import struct
import sys
from io import BytesIO
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

CLOTH = 0.38  # mean linear luminance of the cloth patches
GEAR = 0.18  # and of everything else
GEAR_FLOOR = 0.06  # the darkest a gear texel goes, linear
DESATURATE = 0.7  # share of the cloth's chroma taken out
MASK_BLUR = 1.5  # px, before the hue test
MASK_TRIANGLE = 0.35  # smoothed mask at the centroid above which a triangle is cloth
LUMA = np.array([0.2126, 0.7152, 0.0722])


def to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def to_srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def read_glb(path):
    data = Path(path).read_bytes()
    jlen = struct.unpack("<I", data[12:16])[0]
    doc = json.loads(data[20 : 20 + jlen])
    start = 20 + jlen + 8
    blen = struct.unpack("<I", data[20 + jlen : 24 + jlen])[0]
    return doc, data[start : start + blen]


def view_bytes(doc, blob, index):
    v = doc["bufferViews"][index]
    o = v.get("byteOffset", 0)
    return blob[o : o + v["byteLength"]]


def write_glb(path, doc, blob):
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    blob += b"\0" * (-len(blob) % 4)
    total = 12 + 8 + len(text) + 8 + len(blob)
    out = b"glTF" + struct.pack("<II", 2, total)
    out += struct.pack("<I", len(text)) + b"JSON" + text
    out += struct.pack("<I", len(blob)) + b"BIN\0" + blob
    Path(path).write_bytes(out)


def main(src, dst):
    doc, blob = read_glb(src)
    prim = doc["meshes"][0]["primitives"]
    if len(prim) != 1 or len(doc["materials"]) != 1:
        sys.exit("already split: start from the generator's glb")
    prim = prim[0]
    image_view = doc["images"][0]["bufferView"]
    img = Image.open(BytesIO(view_bytes(doc, blob, image_view))).convert("RGBA")
    rgba = np.asarray(img).astype(np.float64) / 255
    rgb, alpha = rgba[..., :3], rgba[..., 3]

    blurred = np.asarray(img.convert("RGB").filter(ImageFilter.GaussianBlur(MASK_BLUR))).astype(float) / 255
    mask = ((blurred[..., 2] - blurred[..., 0]) > 0.008) & (blurred.max(-1) > 0.13)
    soft = np.asarray(Image.fromarray((mask * 255).astype("uint8")).filter(ImageFilter.GaussianBlur(2))).astype(float) / 255
    covered = alpha >= 0.5

    lin = to_linear(rgb)
    lum = lin @ LUMA
    cloth = mask & covered
    gear = ~mask & covered
    before = float(lum[covered].mean())
    m_cloth, m_gear = float(lum[cloth].mean()), float(lum[gear].mean())

    # Cloth: scale to CLOTH, take some chroma out (towards its own luminance).
    cloth_lin = lin * (CLOTH / m_cloth)
    cloth_lum = (cloth_lin @ LUMA)[..., None]
    cloth_lin = cloth_lum + (cloth_lin - cloth_lum) * (1 - DESATURATE)
    # Gear: a floor and a gain so its mean is GEAR, hue kept.
    gain = (GEAR - GEAR_FLOOR) / m_gear
    hue = lin / np.maximum(lum[..., None], 1e-4)
    gear_lin = lin * gain + GEAR_FLOOR * np.clip(hue, 0.6, 1.4)
    w = soft[..., None]
    out = np.clip(w * cloth_lin + (1 - w) * gear_lin, 0, 1)
    after = float((out @ LUMA)[covered].mean())
    new = np.dstack([to_srgb(out), alpha])
    buf = BytesIO()
    Image.fromarray((new * 255 + 0.5).astype("uint8"), "RGBA").save(buf, "PNG", optimize=True)

    # Triangles: cloth where the smoothed mask at the centroid is high.
    acc = doc["accessors"]
    idx_acc = acc[prim["indices"]]
    dtype = {5121: "u1", 5123: "<u2", 5125: "<u4"}[idx_acc["componentType"]]
    idx_view = doc["bufferViews"][idx_acc["bufferView"]]
    raw = view_bytes(doc, blob, idx_acc["bufferView"])
    off = idx_acc.get("byteOffset", 0)
    tris = np.frombuffer(raw, dtype=dtype, count=idx_acc["count"], offset=off).reshape(-1, 3)
    uv_acc = acc[prim["attributes"]["TEXCOORD_0"]]
    uv_raw = view_bytes(doc, blob, uv_acc["bufferView"])
    stride = doc["bufferViews"][uv_acc["bufferView"]].get("byteStride", 8)
    uvs = np.stack(
        [
            np.frombuffer(uv_raw, "<f4", count=2, offset=uv_acc.get("byteOffset", 0) + i * stride)
            for i in range(uv_acc["count"])
        ]
    )
    h, wd = soft.shape
    cen = uvs[tris].mean(axis=1)
    px = np.clip((cen[:, 0] % 1.0 * wd).astype(int), 0, wd - 1)
    py = np.clip((cen[:, 1] % 1.0 * h).astype(int), 0, h - 1)
    is_cloth = soft[py, px] > MASK_TRIANGLE
    cloth_idx = tris[is_cloth].astype(dtype).tobytes()
    gear_idx = tris[~is_cloth].astype(dtype).tobytes()

    # Rebuild the binary: the image view and the index view are replaced.
    views, pieces = doc["bufferViews"], []
    new_views = []
    cursor = 0
    for i, v in enumerate(views):
        if i == image_view:
            data = buf.getvalue()
        elif i == idx_acc["bufferView"]:
            data = cloth_idx + gear_idx
        else:
            data = view_bytes(doc, blob, i)
        pad = -cursor % 4
        pieces.append(b"\0" * pad)
        cursor += pad
        nv = dict(v)
        nv["byteOffset"], nv["byteLength"] = cursor, len(data)
        new_views.append(nv)
        pieces.append(data)
        cursor += len(data)
    doc["bufferViews"] = new_views
    doc["buffers"][0]["byteLength"] = cursor
    size = np.dtype(dtype).itemsize
    cloth_acc = dict(idx_acc, byteOffset=0, count=int(is_cloth.sum()) * 3)
    gear_acc = dict(idx_acc, byteOffset=len(cloth_idx), count=int((~is_cloth).sum()) * 3)
    for a in (cloth_acc, gear_acc):
        a.pop("min", None)
        a.pop("max", None)
    acc[prim["indices"]] = cloth_acc
    acc.append(gear_acc)
    first = dict(prim, indices=prim["indices"], material=0)
    second = dict(prim, indices=len(acc) - 1, material=1)
    doc["meshes"][0]["primitives"] = [first, second]
    base = doc["materials"][0]
    doc["materials"] = [dict(base, name="Cloth"), dict(base, name="Gear")]
    write_glb(dst, doc, b"".join(pieces))
    record = {
        "tool": "tools/character_values.py",
        "why": "review step six, F1 item 4: the body drew at a mean luminance of 0.02 and the household colour multiplied it",
        "input_mean": before,
        "output_mean": after,
        "cloth_share_of_texels": float(cloth.sum() / covered.sum()),
        "cloth_mean_before": m_cloth,
        "gear_mean_before": m_gear,
        "cloth_target": CLOTH,
        "gear_target": GEAR,
        "gear_floor": GEAR_FLOOR,
        "desaturate": DESATURATE,
        "triangles": {"cloth": int(is_cloth.sum()), "gear": int((~is_cloth).sum())},
        "materials": ["Cloth", "Gear"],
        "note": "the generator was not rerun; frontier_ranger.bundle.json still names the generator's body",
    }
    Path(dst).with_suffix(".values.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main(*sys.argv[1:3])
