#!/usr/bin/env python3
"""Capture the four look fixtures and record what a look costs (lighting plan, Phase 0).

    just look --label "Phase 1 sun" --render-look baseline
    just look --label "Phase 1 sun" --fixtures canopy dawn
    just look --compare "Phase 0 baseline" "Phase 1 sun"

Each fixture runs 400 frames under xvfb at 1600x900, saves its last frame to
scratch/look/<label>-<fixture>.png and has the game write the frame summary
(`--bench-out`). The table (GPU mean, render and main p95) is printed and
appended to docs/perf/look-ledger.md under the label and date. The look
fixtures judge a frame, not the frame rate, so the numbers are for diffing two
labels from one session (`--compare`), not for the work gates of `just bench`.

Requires a built release game (`cargo build --release -p game`).
"""
import argparse
import datetime
import json
import math
import os
import platform
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "docs/perf/look-ledger.md"
FRAMES = 400
TILT = math.radians(35.0)  # DayCycle::default().tilt


def sun_yaw(fraction):
    """The `--look` yaw that faces the sun at a day fraction.

    DayCycle::sun_direction is (-sin h, cos h cos t, -cos h sin t) with
    h = (fraction - 0.5) tau; the camera at yaw y looks along (-sin y, -cos y)
    in (x, z), so facing the sun is atan2(-x, -z) of its horizontal part."""
    hour = (fraction - 0.5) * math.tau
    x = -math.sin(hour)
    z = -math.cos(hour) * math.sin(TILT)
    return math.atan2(-x, -z)


def sun_elevation(fraction):
    hour = (fraction - 0.5) * math.tau
    return math.asin(math.cos(hour) * math.cos(TILT))


CANOPY_TIME = 0.333
# Eye height under the trees, looking at the sun: the pitch lifts the view to
# the sun's elevation less a few degrees so the disc sits in the upper third.
CANOPY_PITCH = sun_elevation(CANOPY_TIME) - 0.12
# Port Serein's street faces the sun up the terraces: at 0.73 (17:30, the
# plan's hour) it is six degrees up and already behind the hill, so the
# fixture takes 0.70, fifteen degrees, where the disc clears the market's
# tree and the crown carves the rays, framed like the canopy.
DUSK_TIME = 0.70
DUSK_PITCH = sun_elevation(DUSK_TIME) - 0.12


def still(altitude, pitch, yaw):
    return ["--flycam", "--lock-camera", "--camera-altitude", str(altitude), f"--camera-pitch={pitch:.3f}", f"--look={yaw:.4f}"]


# `--visual-time` freezes the rendered sun at the fraction, so every fixture
# is a locked camera under a still sun and two captures of one label agree.
FIXTURES = {
    "canopy": ["--study-scene", "woodland", *still(1.8, CANOPY_PITCH, sun_yaw(CANOPY_TIME)), "--time", str(CANOPY_TIME), "--visual-time", str(CANOPY_TIME)],
    "dawn": ["--study-scene", "valley", *still(10, 0.0, 1.5708), "--time", "0.27", "--visual-time", "0.27", "--fog", "1", "--weather", "1,0.5"],
    "dusk-street": ["--at", "city", *still(1.8, DUSK_PITCH, sun_yaw(DUSK_TIME)), "--time", str(DUSK_TIME), "--visual-time", str(DUSK_TIME)],
    "coast": ["--study-scene", "coast", *still(1.8, -0.05, -1.25), "--time", "0.5", "--visual-time", "0.5"],
}


def slug(text):
    return re.sub(r"[^A-Za-z0-9]+", "-", text).strip("-").lower()


def run(fixture, args, config):
    out = ROOT / "scratch/look"
    out.mkdir(parents=True, exist_ok=True)
    name = f"{slug(args.label)}-{fixture}"
    summary = out / f"{name}.json"
    summary.unlink(missing_ok=True)
    command = [
        str(args.binary),
        *FIXTURES[fixture],
        "--frames", str(FRAMES),
        "--window", args.window,
        "--render-look", args.render_look,
        "--wind", "0",
        "--screenshot", str(out / f"{name}.png"),
        "--shot-frames", str(FRAMES),
        "--bench-warmup", str(args.warmup),
        "--bench-out", str(summary),
    ]
    if "--fog" not in command:
        command += ["--fog", "0"]
    width, height = args.window.split("x")
    command = ["xvfb-run", "-a", "-s", f"-screen 0 {width}x{height}x24", *command]
    env = dict(os.environ, XDG_CONFIG_HOME=str(config))
    with (out / f"{name}.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=600, check=False)
    if result.returncode or not summary.exists():
        return None
    return json.loads(summary.read_text())


def row(fixture, m):
    if m is None:
        return f"| {fixture} | run failed | | |"
    return f"| {fixture} | {m['gpu_mean_ms']:.2f} | {m.get('render_p95_ms', 0):.2f} | {m.get('main_p95_ms', 0):.2f} |"


def capture(args):
    if not args.binary.exists():
        sys.exit(f"{args.binary} is missing; run `cargo build --release -p game` first")
    config = ROOT / "scratch/look/config"
    settings = config / "sci-fi-scrolls/settings.ron"
    settings.parent.mkdir(parents=True, exist_ok=True)
    settings.write_text('(values: {"settings.fov": 45.0, "settings.foliage_distance": 100.0, "settings.show_hud": false, "settings.ui_scale": "100"})\n')
    lines = [
        "| Fixture | GPU mean ms | Render p95 ms | Main p95 ms |",
        "| --- | --- | --- | --- |",
    ]
    for fixture in args.fixtures:
        print(f"{fixture} ...", flush=True)
        measured = run(fixture, args, config)
        if measured is None:
            print(f"  run failed; see scratch/look/{slug(args.label)}-{fixture}.log", flush=True)
        lines.append(row(fixture, measured))
    gpu = subprocess.run(["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"], capture_output=True, text=True, check=False)
    header = (
        f"\n## {args.label}\n\n"
        f"{datetime.date.today().isoformat()}, {gpu.stdout.strip() or 'GPU unknown'}, {platform.system()}, "
        f"{args.window}, release, `{args.render_look}` look, virtual display (wall clock not meaningful), "
        f"{FRAMES} frames, {args.warmup:.0f} s warm-up. Captures in scratch/look/{slug(args.label)}-*.png.\n\n"
    )
    text = header + "\n".join(lines) + "\n"
    print(text)
    if not LEDGER.exists():
        LEDGER.write_text(
            "# Look ledger\n\nGPU cost of the four look fixtures of `tools/look.py` "
            "(the lighting plan, `docs/research/lighting-fidelity.md`), appended by "
            "`just look --label \"<text>\"`. Compare two sections with "
            "`just look --compare <a> <b>`; numbers are for diffs within a session.\n"
        )
    with LEDGER.open("a") as ledger:
        ledger.write(text)
    print(f"recorded in {LEDGER.relative_to(ROOT)}")


def sections():
    """Label to {fixture: gpu ms}; the last section of a label wins."""
    found = {}
    label = None
    for line in LEDGER.read_text().splitlines():
        if line.startswith("## "):
            label = line[3:].strip()
            found[label] = {}
        elif label and line.startswith("| ") and not line.startswith(("| Fixture", "| ---")):
            cells = [c.strip() for c in line.strip("|").split("|")]
            try:
                found[label][cells[0]] = float(cells[1])
            except ValueError:
                pass
    return found


def compare(a, b):
    if not LEDGER.exists():
        sys.exit(f"{LEDGER.relative_to(ROOT)} does not exist yet")
    found = sections()
    for label in (a, b):
        if label not in found:
            sys.exit(f"no section {label!r} in the ledger; have {', '.join(found) or 'none'}")
    print(f"| Fixture | {a} GPU ms | {b} GPU ms | Delta ms |\n| --- | --- | --- | --- |")
    for fixture in FIXTURES:
        if fixture in found[a] and fixture in found[b]:
            x, y = found[a][fixture], found[b][fixture]
            print(f"| {fixture} | {x:.2f} | {y:.2f} | {y - x:+.2f} |")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--label", help="Name of this capture; the ledger section and the PNG prefix")
    parser.add_argument("--render-look", default="baseline", help="Look preset (see `game --render-look`)")
    parser.add_argument("--fixtures", nargs="+", default=list(FIXTURES), choices=list(FIXTURES))
    parser.add_argument("--compare", nargs=2, metavar=("A", "B"), help="Print the per-fixture GPU delta between two ledger labels")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/game")
    parser.add_argument("--window", default="1600x900")
    parser.add_argument("--warmup", type=float, default=5.0, help="Seconds of streaming before measuring")
    args = parser.parse_args()
    if args.compare:
        compare(*args.compare)
        return
    if not args.label:
        parser.error("--label is required")
    capture(args)


if __name__ == "__main__":
    main()
