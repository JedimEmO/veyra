#!/usr/bin/env python3
"""Benchmark the fixtures against the performance gates of WORLD_SLICE_PLAN.md.

    just bench                          every fixture, gates checked
    just bench --fixtures woodland      one fixture
    just bench --record "Phase 6 baseline"   also append the results to docs/perf/ledger.md
    just bench --xvfb                   on a virtual display, gated on work time

Wall-clock frame times on a desktop measure the compositor as often as the
game: a 60 Hz monitor, an unfocused window. Every run therefore also records
the frame's work: the main world's CPU time, the render world's CPU time less
its wait for a swapchain image, and the GPU time. A frame can take no less
than the largest of the three. `--xvfb` runs on a virtual display (no window
on the desktop, presentation far slower than the game) and gates on that
work bound instead of the wall clock.

Requires a built release game (`cargo build --release -p game`), a GPU and a
display. Each run pins the settings a saved user preference could change, lets
the world stream in for the warm-up, and has the game write every frame after
it as a JSON summary (`--bench-out`). Logs and summaries land in scratch/bench/
(or --out, so two people measuring in turn do not overwrite each other's).
The exit status is non-zero when a gate is broken, unless --no-fail.

The warm-up of a fixture that stands still lasts until the foliage streamer is
idle (`--bench-warm idle`: nothing in flight and nothing placed for two
seconds, after --warmup seconds and by --warmup-ceiling at the latest), and
the game then measures a fixed number of frames. A fixed ten seconds measured
the far trees filling in, about 50 s around a new camera
(docs/perf/profile-2026-10-03.md), so the static fixtures' p95 was mostly
streaming.

A moving fixture holds its observer at the start (`--bench-warm hold`) until
the terrain is idle too (no region waiting for a mesh, no meshing task held)
within the same floor and ceiling, then starts the motion and measures from
--lead seconds after it. With the fixed warm-up the walks measured mostly their
first terrain fill (31 to 37 s around the new camera,
docs/perf/streaming-2026-10-04.md), and the walk's character had stopped
against Port Serein's wall: a moving run whose observer covered less than 1 m/s
over the measured stretch, or under 5 m in any 5 s of it, fails. `--warm fixed`
gives every fixture the old warm-up and frame counts, and is what a binary
built before 2026-10-03 (no `--bench-warm`) gets regardless; one built before
2026-10-04 (no `hold`) gets the fixed warm-up for its moving fixtures.
"""
import argparse
import datetime
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "docs/perf/ledger.md"

# The four look-study scenes, standing at eye height looking along the ground;
# a scripted flight into the mountains; and a character walking from the spawn.
STILL = ["--flycam", "--lock-camera", "--camera-altitude", "1.8", "--camera-pitch=-0.05", "--look=-1.25"]
FIXTURES = {
    "woodland": ["--study-scene", "woodland", *STILL, "--frames", "4000"],
    "grove": ["--study-scene", "grove", *STILL, "--frames", "4000"],
    "valley": ["--study-scene", "valley", *STILL, "--frames", "4000"],
    "coast": ["--study-scene", "coast", *STILL, "--frames", "4000"],
    # From the plains at (15000, -12600), 155 m, flying towards -x along
    # the foothills into the range across Port Serein's south (on the
    # worldmap), ending at 1 398 m in the mountains. Until ticket 0004's step 2
    # it flew from (20000, -5000) at heading -2.2, which the step put inside
    # the central massif; after step 2 from (7500, 30000) at heading -1.963,
    # which the review's taller mountains brought to 0.3 m over the ground at
    # its end; after the review from (35000, 40000) at heading 1.178, which
    # step 2b's ranges put on a mountainside (re-simulated with the flight's
    # own law, 170 m under the ground at 7.6 s). From (15000, -12500) it came
    # no nearer the ground than 56 m on step 2b's ground, 51.9 m on step 4's,
    # which gave the ranges a foot, and 52.4 m since that step's review; step
    # 5's taller crests brought that start to 39.3 m over a spur at 34.3 s, so
    # it starts 100 m south, which comes no nearer than 43.0 m (at 33.7 s,
    # over 677 m; re-simulated, the ticket's fixture table).
    "flight": ["--flight", "--flight-heading", "1.5708", "--spawn", "15000,-12600", "--flight-secs", "70"],
    # West across the wooded hills inland of Port Serein, from (-1200, -200)
    # at walking pace. Until 2026-10-04 it walked north from the default
    # spawn, (-500, -250) on the beach, and stopped 78 m later against the
    # city wall at (-494, -328) (`blockout-walls.ashlar`, its face 80 degrees
    # from up), so the fixture measured a static view under a moving name.
    # This line was walked for 133 s and 667 m with nothing in its way.
    "walk": ["--auto-walk", "--spawn=-1200,-200", "--look", "1.5708", "--frames", "6000"],
    # Port Serein (Phase 11): on its street, above its roofs, and from the far
    # end of its road looking at it across the wilderness.
    "city-street": ["--at", "city", "--look", "3.14159", "--frames", "4000"],
    "city-rooftop": ["--at", "city", "--flycam", "--lock-camera", "--camera-altitude", "90", "--camera-pitch=-0.15", "--look", "2.4", "--frames", "4000"],
    "city-approach": ["--at", "road", "--flycam", "--lock-camera", "--camera-altitude", "1.8", "--camera-pitch=-0.02", "--look", "2.64", "--frames", "4000"],
    # The slice's route (Phase 12): the road into the city, sprinting. The
    # walker stops about 125 s in, at (-691, -978) between waypoints 46 and
    # 47 of 101, against a 2.5 m step in the ground across the road (the
    # ground collider of region (-11, -16), its face 78 degrees from up), so
    # it never reaches the city; the measured stretch ends before that.
    "route-walk": ["--route", "--frames", "6000"],
    # The terraced Port Serein: from the bay, the whole city in view, and
    # among the Terraces' houses.
    "city-bay": ["--flycam", "--lock-camera", "--spawn=-146,-289", "--camera-altitude", "50", "--camera-pitch=-0.08", "--look", "0.877", "--frames", "3000"],
    "city-terraces": ["--flycam", "--lock-camera", "--spawn=-542,-424", "--camera-altitude", "22", "--camera-pitch=-0.28", "--look", "0.873", "--frames", "3000"],
}
# Fixtures that move, where a streaming stall shows as a long frame.
MOVING = {"flight", "walk", "route-walk"}

# Seconds a held walk measures, after its lead: the route-walk's walker stops
# at the step about 125 s after it starts.
MOVING_SECS = 100.0

# A moving run fails when its observer went slower than this over the
# measured stretch, metres per second, or covered less than STALL_M in any
# five seconds of it.
MIN_SPEED_MPS = 1.0
STALL_M = 5.0

# Frames a settled static fixture measures after its warm-up: about what the
# 4000-frame runs measured after ten seconds.
SETTLED_FRAMES = 3000

# Section 3 of the plan. The per-system budget is read from GPU captures by a
# person; these are the gates a script can hold.
GATES = {
    "p95_ms": 8.3,
    "p99_ms": 12.0,
}


def work(measured):
    """The frame's work bound: the larger of the main world's, the render
    world's and the GPU's, at the 95th percentile (the GPU's is a mean)."""
    return max(measured.get("main_p95_ms", 0.0), measured.get("render_p95_ms", 0.0), measured["gpu_mean_ms"])


def settles(fixture, args):
    """Whether this run waits for the streamer to go idle."""
    return args.warm == "idle" and fixture not in MOVING


def holds(fixture, args):
    """Whether this run holds its motion until the streamers are idle."""
    return args.warm == "idle" and args.hold and fixture in MOVING


def knows_settling(binary):
    """Whether the game can wait for the streamer itself: an older binary,
    kept for an A/B against the tree it was built from, cannot."""
    shown = subprocess.run([str(binary), "--help"], capture_output=True, text=True, check=False)
    # A word boundary, not a substring: every older binary has `--bench-warmup`.
    return re.search(r"--bench-warm\b(?!-)", shown.stdout) is not None


def knows_holding(binary):
    """Whether the game can hold a moving fixture at its start: a binary
    built before 2026-10-04 cannot."""
    shown = subprocess.run([str(binary), "--help"], capture_output=True, text=True, check=False)
    return re.search(r"possible values: fixed, idle, hold", shown.stdout) is not None


def fixture_args(fixture, args):
    """The fixture's arguments, with a settled run's length in measured frames
    rather than frames from the start, whatever its warm-up took."""
    given = FIXTURES[fixture]
    if not settles(fixture, args) and not holds(fixture, args):
        return list(given)
    kept = []
    skip = False
    for arg in given:
        if skip:
            skip = False
        elif arg == "--frames":
            skip = True
        else:
            kept.append(arg)
    if holds(fixture, args):
        held = [*kept, "--bench-warm", "hold", "--bench-warm-ceiling", str(args.warmup_ceiling), "--bench-lead", str(args.lead)]
        # The flight ends when it has flown --flight-secs from its release.
        return held if "--flight" in kept else [*held, "--bench-secs", str(MOVING_SECS)]
    return [*kept, "--bench-warm", "idle", "--bench-warm-ceiling", str(args.warmup_ceiling), "--bench-frames", str(SETTLED_FRAMES)]


def run(fixture, args, config):
    out = args.out
    out.mkdir(parents=True, exist_ok=True)
    summary = out / f"{fixture}.json"
    summary.unlink(missing_ok=True)
    command = [
        str(args.binary),
        *fixture_args(fixture, args),
        "--window", args.window,
        "--render-look", "baseline",
        # No ground fog pass (ticket 0003), so the numbers stay comparable
        # with the ledger's older ones; ticket 0003's step 5 benches it.
        "--fog", "0",
        "--visual-time", "0.45",
        "--wind", "0",
        # The debug statistics were on by default when the ledger's rows were
        # taken; they stay on here so the measured frame is the same one.
        "--stats",
        "--bench-warmup", str(args.warmup),
        "--bench-out", str(summary),
    ]
    if args.xvfb:
        width, height = args.window.split("x")
        command = ["xvfb-run", "-a", "-s", f"-screen 0 {width}x{height}x24", *command]
    env = dict(os.environ, XDG_CONFIG_HOME=str(config))
    with (out / f"{fixture}.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=600, check=False)
    if result.returncode or not summary.exists():
        return None, command
    measured = json.loads(summary.read_text())
    # The game runs continuously when scripted, focused or not, so a median
    # of a 60 Hz refresh with the GPU far below it means something else is
    # pacing it (a vsync'd display, a compositor): refuse the run rather
    # than record it.
    if not args.xvfb and 16.3 < measured["p50_ms"] < 17.0 and measured["gpu_mean_ms"] < 12.0:
        print(f"  {fixture}: paced at 60 Hz by the display; not a measurement", flush=True)
        return None, command
    return measured, command


def stopped(fixture, measured):
    """Why a moving fixture's observer did not move, if it did not; a
    binary that does not report its travel is not judged."""
    if fixture not in MOVING or "speed_mps" not in measured:
        return []
    failures = []
    if measured["speed_mps"] < MIN_SPEED_MPS:
        failures.append(f"moved {measured['speed_mps']:.2f} m/s < {MIN_SPEED_MPS}")
    slowest = measured.get("slowest_5s_m")
    if slowest is not None and slowest < STALL_M:
        failures.append(f"stopped: {slowest:.1f} m in some 5 s < {STALL_M}")
    return failures


def broken(fixture, measured, xvfb):
    if xvfb:
        # The wall clock is the virtual display's; only the work is ours.
        bound = work(measured)
        failures = [f"work {bound:.2f} > {GATES['p95_ms']}"] if bound > GATES["p95_ms"] else []
        return failures + stopped(fixture, measured)
    failures = [f"{name} {measured[name]:.2f} > {limit}" for name, limit in GATES.items() if measured[name] > limit]
    if fixture in MOVING and measured["over_33ms"]:
        failures.append(f"{measured['over_33ms']} frames over 33 ms")
    return failures + stopped(fixture, measured)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fixtures", nargs="+", default=list(FIXTURES), choices=list(FIXTURES))
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/game")
    parser.add_argument("--out", type=Path, default=ROOT / "scratch/bench", help="Where the logs and JSON summaries go")
    parser.add_argument("--window", default="2560x1440")
    parser.add_argument("--warmup", type=float, default=10.0, help="Seconds of streaming before measuring; the floor of a settling warm-up")
    parser.add_argument("--warm", choices=["idle", "fixed"], default="idle", help="Static fixtures wait for the foliage streamer to go idle (default) or a fixed --warmup")
    parser.add_argument("--warmup-ceiling", type=float, default=90.0, help="The longest a settling warm-up waits, seconds")
    parser.add_argument("--lead", type=float, default=5.0, help="Seconds a held moving fixture moves before it is measured")
    parser.add_argument("--record", metavar="LABEL", help="Append the results to docs/perf/ledger.md under this label")
    parser.add_argument("--no-fail", action="store_true", help="Report broken gates without failing")
    parser.add_argument("--xvfb", action="store_true", help="Run on a virtual display and gate on work time")

    args = parser.parse_args()
    # Absolute: XDG_CONFIG_HOME must be, or the game ignores it and reads the
    # user's own settings (a longer foliage distance, another UI scale).
    args.out = args.out.resolve()
    if not args.binary.exists():
        sys.exit(f"{args.binary} is missing; run `cargo build --release -p game` first")

    if args.warm == "idle" and not knows_settling(args.binary):
        print(f"{args.binary} has no --bench-warm; every fixture gets the fixed {args.warmup:.0f} s warm-up", flush=True)
        args.warm = "fixed"
    args.hold = args.warm == "idle" and knows_holding(args.binary)
    if args.warm == "idle" and not args.hold:
        print(f"{args.binary} has no --bench-warm hold; the moving fixtures get the fixed {args.warmup:.0f} s warm-up", flush=True)

    config = args.out / "config"
    settings = config / "sci-fi-scrolls/settings.ron"
    settings.parent.mkdir(parents=True, exist_ok=True)
    # The UI at the design's own pixels, as the ledger's rows were taken.
    settings.write_text('(values: {"settings.fov": 45.0, "settings.foliage_distance": 100.0, "settings.show_hud": true, "settings.ui_scale": "100"})\n')

    rows = []
    failed = False
    restore = hold_gpu_clock()
    try:
        failed = measure(args, config, rows)
    finally:
        restore()

    if args.record:
        record(args, rows)
    if failed and not args.no_fail:
        sys.exit("a gate is broken")


def hold_gpu_clock():
    """Holds an NVIDIA card at full clock for the run; returns the undo.

    Under a virtual display a frame is paced by the present, so the card
    idles most of it and its governor drops the clocks: the same frame then
    reads two to four times the GPU time, and more the faster the CPU side
    gets. `GpuPowerMizerMode` 1 is "prefer maximum performance"; it is set
    through the desktop's own X display and put back when the run ends.
    Without `nvidia-settings` or a desktop display the run goes on as it is.
    """
    display = os.environ.get("DISPLAY")
    attribute = "[gpu:0]/GpuPowerMizerMode"
    if not display:
        return lambda: None
    try:
        shown = subprocess.run(["nvidia-settings", "-t", "-q", attribute], capture_output=True, text=True, check=False, timeout=10)
        before = shown.stdout.strip()
        if shown.returncode != 0 or not before.isdigit():
            return lambda: None
        subprocess.run(["nvidia-settings", "-a", f"{attribute}=1"], capture_output=True, check=False, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return lambda: None
    print(f"GPU held at maximum performance for the run (was mode {before})", flush=True)

    def restore():
        subprocess.run(["nvidia-settings", "-a", f"{attribute}={before}"], capture_output=True, check=False, timeout=10)

    return restore


def measure(args, config, rows):
    """Runs every fixture, fills `rows`, and says whether a gate broke."""
    failed = False
    for fixture in args.fixtures:
        print(f"{fixture} ...", flush=True)
        measured, command = run(fixture, args, config)
        if measured is None:
            print(f"  run failed; see {args.out / fixture}.log", flush=True)
            rows.append((fixture, None, ["run failed"]))
            failed = True
            continue
        failures = broken(fixture, measured, args.xvfb)
        failed |= bool(failures)
        rows.append((fixture, measured, failures))
        print(
            f"  p50 {measured['p50_ms']:.2f}  p95 {measured['p95_ms']:.2f}  p99 {measured['p99_ms']:.2f}"
            f"  worst {measured['worst_ms']:.1f}  >33ms {measured['over_33ms']}  gpu {measured['gpu_mean_ms']:.2f}"
            f"  main p95 {measured.get('main_p95_ms', 0):.2f}  render p50 {measured.get('render_p50_ms', 0):.2f}"
            f"  render p95 {measured.get('render_p95_ms', 0):.2f}  main max {measured.get('main_max_ms', 0):.1f}"
            f"  render max {measured.get('render_max_ms', 0):.1f}  entities {measured['entities']}"
            f"  foliage {foliage(measured)}  warm-up {warm_up(measured)}{streaming(measured)}"
            f"  {'; '.join(failures) or 'gates ok'}",
            flush=True,
        )
    return failed


def foliage(measured):
    """Foliage entities, the median of the cost line over the measured stretch."""
    value = measured.get("foliage_entities")
    return "" if value is None else f"{value:.0f}"


def warm_up(measured):
    """How long the warm-up took and how it ended (`settled`, `ceiling` or
    `fixed`), and for a held run when its motion started."""
    seconds = measured.get("warmup_s")
    if seconds is None:
        return ""
    held = measured.get("held_s")
    if held is not None:
        return f"held {held:.0f} s {measured.get('warm_up', '')}"
    return f"{seconds:.0f} s {measured.get('warm_up', '')}".strip()


def streaming(measured):
    """The streamers and the observer over the measured stretch, for a run
    whose binary reports them."""
    if "terrain_uploads_per_frame" not in measured:
        return ""
    text = (
        f"  terrain {measured['terrain_uploads_per_frame']:.2f}/frame ({measured['terrain_uploads_per_s']:.1f}/s,"
        f" max {measured['terrain_uploads_max_frame']}), tasks {measured['terrain_tasks_mean']:.0f}"
        f" (max {measured['terrain_tasks_max']}), pending {measured['regions_pending_mean']:.0f},"
        f" cancelled {measured['terrain_cancelled_per_s']:.1f}/s"
        f"  colliders {measured['heightfields_per_s']:.1f} ground/s, +{measured['colliders_added_per_s']:.0f}"
        f" -{measured['colliders_removed_per_s']:.0f}/s"
    )
    if "speed_mps" in measured:
        text += f"  moved {measured['travel_m']:.0f} m at {measured['speed_mps']:.2f} m/s"
        if "slowest_5s_m" in measured:
            text += f" (slowest 5 s {measured['slowest_5s_m']:.1f} m)"
    return text


def column(measured, key, digits):
    """A value of the summary for the ledger, blank when the binary did not write it."""
    value = measured.get(key)
    return "" if value is None else f"{value:.{digits}f}"


def record(args, rows):
    """Append one dated section to the ledger: the numbers each phase exits on."""
    gpu = subprocess.run(["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"], capture_output=True, text=True, check=False)
    if args.warm == "idle":
        moving = (
            f"moving fixtures held at their start until terrain and foliage are idle (same bounds), then "
            f"measured from {args.lead:.0f} s after they start ({MOVING_SECS:.0f} s for the walks, the flight to its end)"
            if args.hold
            else f"moving fixtures {args.warmup:.0f} s"
        )
        warm = (
            f"static fixtures warmed until the foliage streamer is idle ({args.warmup:.0f} to "
            f"{args.warmup_ceiling:.0f} s) then {SETTLED_FRAMES} frames measured, {moving}"
        )
    else:
        warm = f"{args.warmup:.0f} s warm-up"
    lines = [
        f"\n## {args.record}\n",
        f"{datetime.date.today().isoformat()}, {gpu.stdout.strip() or 'GPU unknown'}, {platform.system()}, "
        f"{args.window}, release, TAA + SSAO (`baseline` look), no ground fog (`--fog 0`), {warm}"
        f"{', virtual display (wall clock not meaningful)' if args.xvfb else ''}.\n",
        "| Fixture | Frames | p50 ms | p95 ms | p99 ms | Worst ms | Over 33 ms | GPU ms | Main p95 ms | Render p50 ms | Render p95 ms | Main max ms | Render max ms | Terrain uploads a frame | Moved m/s | Entities | Foliage entities | Warm-up | Gates |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for fixture, measured, failures in rows:
        if measured is None:
            lines.append(f"| {fixture} | | | | | | | | | | | | | | | | | | run failed |")
            continue
        lines.append(
            f"| {fixture} | {measured['frames']} | {measured['p50_ms']:.2f} | {measured['p95_ms']:.2f} | "
            f"{measured['p99_ms']:.2f} | {measured['worst_ms']:.1f} | {measured['over_33ms']} | "
            f"{measured['gpu_mean_ms']:.2f} | {measured.get('main_p95_ms', 0):.2f} | "
            f"{measured.get('render_p50_ms', 0):.2f} | {measured.get('render_p95_ms', 0):.2f} | "
            f"{column(measured, 'main_max_ms', 1)} | {column(measured, 'render_max_ms', 1)} | "
            f"{column(measured, 'terrain_uploads_per_frame', 2)} | "
            f"{column(measured, 'speed_mps', 2) if fixture in MOVING else ''} | "
            f"{measured['entities']} | {foliage(measured)} | {warm_up(measured)} | {'; '.join(failures) or 'ok'} |"
        )
    with LEDGER.open("a") as ledger:
        ledger.write("\n".join(lines) + "\n")
    print(f"recorded in {LEDGER.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
