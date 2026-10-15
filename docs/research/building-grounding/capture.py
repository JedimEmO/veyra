#!/usr/bin/env python3
"""Print one capture command. --run explicitly launches it; no batch rendering."""
import argparse
import math
import os
from pathlib import Path
import re
import shlex
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--site", required=True, choices=["flat", "slope", "uneven"])
parser.add_argument("--view", required=True, choices=["overview", "ground"])
parser.add_argument("--run", action="store_true", help="Launch this single GPU capture")
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
output = Path(__file__).resolve().parent
# The sites are the game's own (apps/game/src/grounding.rs), read from there
# so that the camera lands on the ground the game fits the outpost to; they
# moved in ticket 0004's step 2b, when a range took the plateau they stood on.
_source = (root / "apps/game/src/grounding.rs").read_text()
sites = {
    name: (float(x), float(z), float(yaw))
    for name, x, z, yaw in re.findall(
        r'\("(\w+)", \[(-?[\d.]+), (-?[\d.]+)\], (-?[\d.]+)\)', _source)
}
assert set(sites) == {"flat", "slope", "uneven"}, sites
views = {"overview": (-10, -18, 12, -.34, -2.494), "ground": (14, -9, 1.8, .015, 2.51)}
x, z, yaw = sites[args.site]
cx, cz, altitude, pitch, heading = views[args.view]
if args.site == "uneven" and args.view == "ground":
    cz, heading = -13, 2.65  # Keep the longer ramp toe inside the frame.
sx = x + math.cos(yaw)*cx + math.sin(yaw)*cz
sz = z - math.sin(yaw)*cx + math.cos(yaw)*cz
command = ["taskset", "-c", "0-3", "nice", "-n", "10", "./target/debug/game",
           "--serial-pipelines", "--grounding-study", args.site, "--flycam", "--lock-camera",
           f"--spawn={sx},{sz}", f"--look={heading+yaw}",
           "--camera-altitude", str(altitude), f"--camera-pitch={pitch}",
           "--window", "1600x900", "--render-look", "baseline", "--visual-time", "0.62",
           "--frames", "420", "--screenshot", str(output / f"{args.site}-{args.view}.png")]
print(shlex.join(command), flush=True)
if args.run:
    # Persist crash logs across reboot. There is deliberately no automatic retry.
    with open(output / "last-capture.log", "w") as log:
        subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=300,
                       env={**os.environ, "RUST_MIN_STACK": "8388608"})
