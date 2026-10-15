"""Explicit Forge recipes for the initial combat study's existing ARDY takes.

No new ARDY samples are claimed: upstream sidecars are preserved beside the
imported takes. Forge records these re-bakes as reconstructed. Rerunning this
script re-promotes derived clips through the same supported door.
"""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
RECIPES = [
    ("run", "scrapyard_rifle_run", 1.55, 2.6, .1, "", []),
    ("aim", "scrapyard_rifle_aim", .5, 3.45, .1, "", []),
    ("fire", "scrapyard_rifle_fire", .45, 2.15, 0, "0:0,0.2:0.035,0.65:0.08,1.35:0.15", [(0.65,"shot")]),
    ("light", "ruin_explorer_attack", .25, 2.85, 0, "0:0,0.2:0.13,0.4:0.22,0.65:0.35,0.85:0.40", [(.45,"contact_start"),(.65,"contact_end")]),
    ("followup", "sword_chop", .15, 1.2, 0, "0:0,0.6:0.12,0.95:0.22,1.6:0.40", [(.75,"contact_start"),(1.1,"contact_end")]),
    ("heavy", "sword_chop", 0, 1.2, 0, "0:0,0.75:0.32,1.1:0.45,1.75:0.85", [(.75,"contact_start"),(1.1,"contact_end")]),
    ("dodge", "roll", .2, 0, 0, "0:0,0.5:0.12,1.05:0.30,1.75:0.48", []),
    ("death", "death", .5, 1.55, 0, "", []),
    ("walk", "walk", 3.95, 1.4, .1, "", []),
]

for name, source, start, end, blend, retime, events in RECIPES:
    command = ["bash", str(ROOT/"tools/combat_forge.sh"), "promote", "clip", f"assets-src/imported/{source}.npz", name,
               "--trim-start", str(start), "--trim-end", str(end), "--in-place", "strip", "--y-mode", "off",
               "--loop" if blend else "--no-loop", "--loop-blend", str(blend),
               "--exaggerate", "1", "--arm-bend", "0", "--lean", "0", "--shoulder-back", "0",
               "--retime", retime, "--created-by", "agent:codex", "--overwrite",
               "--note", f"Re-baked existing ARDY take; upstream receipt in assets-src/imported/{source}.upstream.json. Timing study, 2026-10-01."]
    for time, label in events: command += ["--event", f"{time}:{label}"]
    subprocess.run(command, cwd=ROOT, check=True)
