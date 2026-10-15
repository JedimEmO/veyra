#!/usr/bin/env python3
"""Print a reproducible single capture; --run starts the GPU application."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("view", choices=["tower", "annex", "block", "world"])
parser.add_argument("--run", action="store_true")
args = parser.parse_args()
output = Path(__file__).resolve().parent
root = output.parents[2]
command = ["taskset", "-c", "0-3", "nice", "-n", "10"]
if args.view == "world":
    command += ["./target/debug/game", "--serial-pipelines", "--grounding-study", "flat",
                "--grounding-style", "corporate", "--flycam", "--lock-camera",
                "--spawn=-1440,-12230", "--look=-2.48", "--camera-altitude", "22",
                "--camera-pitch=-0.26", "--window", "1600x900", "--render-look", "baseline",
                "--visual-time", "0.62", "--frames", "420"]
else:
    scene = {"tower": "corporate", "annex": "corporate-annex", "block": "corporate-block"}[args.view]
    command += ["./target/debug/building-preview", "--serial-pipelines", "--scene", scene,
                "--zoom", "0.9" if args.view == "tower" else "0.85", "--frames", "60"]
    if args.view == "block":
        command += ["--yaw=-0.35", "--pitch", "0.4"]
command += ["--screenshot", str(output / f"{args.view}.png")]
print(shlex.join(command), flush=True)
if args.run:
    with open(output / "last-capture.log", "w") as log:
        subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT,
                       env={**os.environ, "RUST_MIN_STACK": "8388608"}, check=True, timeout=300)
