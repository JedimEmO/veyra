#!/usr/bin/env python3
"""Capture matched render studies. Requires a built release game and a GPU/display.

python3 tools/render-study.py --output /tmp/render-study --scenes woodland --times .42
python3 tools/render-study.py --output /tmp/render-study-full --scenes woodland grove valley coast --times .32 .5 .68
python3 tools/render-study.py --output /tmp/render-ablation --ablations

Results include full logs, images, exact commands, RON settings and summaries.
The final three logged measurement windows exclude most startup/streaming work;
inspect logs and foliage counts before treating any run as a stable benchmark.
"""
import argparse
import json
import os
import re
import statistics
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PRESETS = ["baseline", "clear-contrast", "sunlit-mist", "cinematic-woodland"]
PATTERN = re.compile(r"([\d.]+) fps, avg ([\d.]+) ms, p50 ([\d.]+) ms, p95 ([\d.]+) ms, p99 ([\d.]+) ms, worst ([\d.]+) ms, gpu ([\d.]+) ms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/game")
    parser.add_argument("--scenes", nargs="+", default=["woodland"], choices=["woodland", "grove", "valley", "coast"])
    parser.add_argument("--times", nargs="+", type=float, default=[.42])
    parser.add_argument("--presets", nargs="+", default=PRESETS, choices=PRESETS)
    parser.add_argument("--frames", type=int, default=2700)
    parser.add_argument("--window", default="2560x1440")
    parser.add_argument("--fov", type=float, default=45.0)
    parser.add_argument("--foliage-distance", type=float, default=100.0, help="Percentage, independent of saved user preferences")
    parser.add_argument("--fullscreen", action="store_true", help="Use the primary monitor; its native size must match --window")
    parser.add_argument("--ablations", action="store_true")
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    if not (40 <= args.fov <= 100 and 25 <= args.foliage_distance <= 200):
        parser.error("FOV must be 40..100 and foliage distance 25..200")
    config = args.output / "config"
    settings = config / "sci-fi-scrolls/settings.ron"
    settings.parent.mkdir(parents=True, exist_ok=True)
    settings.write_text(f'(values: {{"settings.fov": {args.fov}, "settings.foliage_distance": {args.foliage_distance}, "settings.show_hud": true}})\n')
    env = dict(os.environ, XDG_CONFIG_HOME=str(config))
    binary = args.binary.resolve()
    cases = {}
    for preset in args.presets:
        path = args.output / f"{preset}.ron"
        subprocess.run([str(binary), "--render-look", preset, "--export-render-look", str(path)], cwd=ROOT, check=True)
        cases[preset] = path
    if args.ablations:
        source = args.output / "source.ron"
        subprocess.run([str(binary), "--render-look", "clear-contrast", "--export-render-look", str(source)], cwd=ROOT, check=True)
        fields = dict(contact_shadows="false", ssao="false", bloom="0.0", fog_density="0.0", dof="false", motion_blur="0.0")
        cases = {}
        for name, overrides in {"grading": {}, "contact": {"contact_shadows": "true"}, "ssao": {"ssao": "true"}, "bloom": {"bloom": "0.025"}, "fog": {"fog_density": "0.018"}, "dof": {"dof": "true"}, "motion": {"motion_blur": "0.25"}}.items():
            text = source.read_text()
            for key, value in (fields | overrides).items():
                text, count = re.subn(rf"(?m)^(\s*{key}: )[^,]+,", rf"\g<1>{value},", text)
                assert count == 1, key
            path = args.output / f"{name}.ron"
            path.write_text(text)
            cases[name] = path
    results = []
    for scene in args.scenes:
        for time in args.times:
            for name, path in cases.items():
                stem = f"{scene}-{time:.2f}-{name}"
                command = [str(binary), "--study-scene", scene, "--render-look-file", str(path), "--visual-time", str(time), "--flycam", "--lock-camera", "--camera-altitude", "1.8", "--camera-pitch=-0.05", "--look=-1.25", "--aa", "taa", "--wind", "0", "--frames", str(args.frames), "--window", args.window, "--screenshot", str(args.output / f"{stem}.png")]
                if args.fullscreen:
                    command.append("--fullscreen")
                print(stem, flush=True)
                log = args.output / f"{stem}.log"
                shot = args.output / f"{stem}.png"
                shot.unlink(missing_ok=True)
                with log.open("w") as stream:
                    run = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, env=env, timeout=240, check=False)
                dimensions = None
                if shot.exists():
                    with shot.open("rb") as image:
                        image.seek(16)
                        dimensions = struct.unpack(">II", image.read(8))
                expected = tuple(map(int, args.window.split("x")))
                text = log.read_text()
                windows = [list(map(float, values)) for values in PATTERN.findall(text)][-3:]
                result = {"case": stem, "command": command, "environment": {"XDG_CONFIG_HOME": str(config)}, "fov_degrees": args.fov, "foliage_distance_percent": args.foliage_distance, "exit_code": run.returncode, "image_dimensions": dimensions, "last_windows": windows, "gpu_mean_ms": statistics.mean(w[-1] for w in windows) if windows else None, "errors": [line for line in text.splitlines() if "ERROR" in line or "panicked" in line]}
                foliage = re.findall(r"foliage cells=(\d+) blades=(\d+) plants=(\d+) entities=(\d+) in_flight=(\d+)", text)
                result["foliage_final"] = dict(zip(["cells", "blades", "plants", "entities", "in_flight"], map(int, foliage[-1]))) if foliage else None
                results.append(result)
                (args.output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
                if dimensions != expected:
                    raise SystemExit(f"Capture was {dimensions}, expected {expected}; check the window manager or use --fullscreen. {shot}")
                if run.returncode or result["errors"]:
                    raise SystemExit(f"Rendering failed: {log}")
    print(f"Results: {args.output / 'results.json'}", flush=True)


if __name__ == "__main__":
    main()
