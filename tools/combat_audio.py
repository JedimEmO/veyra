#!/usr/bin/env python3
"""Level existing Forge renders for playback. No generators or audio device needed.

Run: python3 tools/combat_audio.py            (level the takes, then encode them)
     python3 tools/combat_audio.py --encode-only   (encode the masters already levelled)

The levelled 48 kHz WAV masters are written to scratch/audio-masters/combat and
are not kept in assets/: the game plays the Ogg Vorbis the last step encodes
from them into assets/audio/combat (a tenth of the bytes, and the download of
a browser session). Outputs include the exact recipe, input/output hashes and
EBU R128 measurements. The retained raw archive is the source when disposable
out/ renders are removed, so the masters are regenerated, not stored.
"""

import argparse
from array import array
import hashlib
import json
import math
from pathlib import Path
import subprocess
import tempfile
import wave
import shutil

ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "assets-src/combat/forge"
DEST = ROOT / "assets/audio/combat"
MASTERS = ROOT / "scratch/audio-masters/combat"
RATE = 48000
# libvorbis quality: transients and the hums are mono, so this is about 90 kbit/s.
OGG_QUALITY = 5
TARGETS = {
    "blaster_shot": -14, "sentry_shot": -14,
    "impact_stone": -16, "impact_metal": -16,
    "impact_shield": -16, "impact_flesh": -16,
    "swing_light": -18, "swing_heavy": -14,
    "blade_hit_armor": -14, "blade_hit_shield": -14,
    "dodge_roll": -24, "reload": -24,
    "foot_stone": -28, "foot_gravel": -28, "foot_metal": -28,
    "jump": -24, "land": -24,
    "sentry_charge": -18, "rusher_windup": -18,
    "enemy_death": -18, "player_hurt": -18,
    "hit_marker": -20, "low_health": -22, "shield_recharge": -22,
    # The interface's own sounds sit well under the fight's.
    "ui_hover": -30, "ui_click": -26, "ui_open": -26, "ui_close": -26,
    "ui_toast": -24, "ui_refuse": -24, "xp_credit": -26,
    "skill_learn": -22, "bench_switch": -22,
    # The field's sounds (ticket 0011): beds under the fight, events at its foot.
    "survey_ping": -22, "survey_found": -22, "survey_empty": -26,
    "sample_loop": -28, "sample_done": -22,
    "bike_hum": -26, "bike_call": -20, "bike_mount": -22, "bike_dismount": -22,
    "bike_thrown": -16,
    "harvester_hum": -30, "harvester_plant": -20, "harvester_collect": -22,
    "craft_roll": -24, "craft_good": -22, "craft_great": -20, "craft_poor": -24,
    "craft_finish": -20, "fit_part": -22, "repair": -22, "sale": -24,
    "map_open": -26, "weapon_swap": -22,
    "guard_break": -14, "blast": -12,
}


def ffmpeg(*args):
    return subprocess.run(
        ["ffmpeg", "-hide_banner", "-nostdin", "-y", *map(str, args)],
        check=True, capture_output=True,
    )


def digest(path):
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def measure(path, duration):
    # BS.1770 needs 400 ms blocks. Repeat short transients for analysis only,
    # rather than mastering their empty padding or boosting a noise floor.
    input_args = ["-stream_loop", "-1", "-i", path, "-t", 3] if duration < .4 else ["-i", path]
    result = ffmpeg(*input_args, "-af", "loudnorm=I=-16:TP=-1:LRA=11:print_format=json",
                    "-f", "null", "-")
    log = result.stderr.decode()
    data = json.loads(log[log.rindex("{"):])
    return {"lufs": float(data["input_i"]), "true_peak_dbfs": float(data["input_tp"])}


def render(source, dest, target, temp, prefilter=""):
    # Work in float before gain: even the quietest retained backend output is usable.
    data = ffmpeg("-i", source, "-af", prefilter + "highpass=f=30", "-ar", RATE,
                  "-ac", 1, "-f", "f32le", "-").stdout
    samples = array("f", data)
    peak = max(map(abs, samples))
    if peak == 0:
        raise ValueError(f"Silent source: {source}")
    # A relative floor preserves quiet takes; keep 5 ms before the first transient.
    threshold = peak * 10 ** (-45 / 20)
    first = next(i for i, x in enumerate(samples) if abs(x) > threshold)
    trim = max(0, first - RATE // 200)
    last = len(samples) - next(i for i, x in enumerate(reversed(samples)) if abs(x) > threshold)
    end = min(len(samples), max(last + RATE // 200, trim + RATE // 100))
    duration = (end - trim) / RATE
    prepare = prefilter + (
        f"highpass=f=30,atrim=start_sample={trim}:end_sample={end},asetpts=PTS-STARTPTS,"
        f"volume={0.8 / peak:.12g}"
    )
    base = temp / "prepared.wav"
    ffmpeg("-i", source, "-af", prepare, "-ac", 1, "-ar", RATE,
           "-c:a", "pcm_f32le", base)
    measured = measure(base, duration)
    gain = target - measured["lufs"]
    if not math.isfinite(gain):
        raise ValueError(f"Unmeasurable source: {source}")
    # Limiting only the transients that cannot fit the requested loudness leaves
    # a true-peak margin, without loudnorm's long look-ahead on tiny one-shots.
    for _ in range(60):
        filters = (f"volume={gain:.8f}dB,"
                   "aresample=192000,alimiter=limit=0.707946:attack=0.1:release=2:level=false:latency=true,aresample=48000")
        pcm = array("f", ffmpeg("-i", base, "-af", filters, "-ar", RATE,
                                "-ac", 1, "-f", "f32le", "-").stdout)
        # Limiting asymmetric transients can add DC. Subtract the weighted mean
        # before the final 5 ms fades so both endpoints and the DC sum are zero.
        fades = [min(1., i / 239, (len(pcm) - 1 - i) / 239) for i in range(len(pcm))]
        dc = sum(x * f for x, f in zip(pcm, fades)) / sum(fades)
        pcm = [(x - dc) * f for x, f in zip(pcm, fades)]
        ceiling = 10 ** (-2 / 20)
        scale = min(1., ceiling / max(map(abs, pcm)))
        with wave.open(str(dest), "wb") as output:
            output.setparams((1, 2, RATE, 0, "NONE", "not compressed"))
            output.writeframes(array("h", (round(x * scale * 32767) for x in pcm)).tobytes())
        measured = measure(dest, duration)
        correction = target - measured["lufs"]
        if abs(correction) <= .2:
            break
        gain += correction
    if abs(correction) > .3 or measured["true_peak_dbfs"] > -0.5:
        raise ValueError(f"Levelling failed: {dest}: {measured}")
    return {"source": str(source.relative_to(ROOT)), "source_sha256": digest(source),
            "output_sha256": digest(dest), "target_lufs": target,
            "trim_samples": trim, "end_sample": end, "duration_s": duration,
            "measurement": "EBU R128" if duration >= .4 else "EBU R128 on 3 s repetition (analysis only)",
            "prepare_filter": prepare,
            "level_filter": filters, "dc_removed": dc, "final_peak_scale": scale, **measured}


def encode(masters):
    """The final step: every levelled WAV master to an Ogg Vorbis in assets/."""
    DEST.mkdir(parents=True, exist_ok=True)
    encoded = {}
    for master in sorted(masters.glob("*.wav")):
        ogg = DEST / (master.stem + ".ogg")
        ffmpeg("-i", master, "-map_metadata", "-1", "-fflags", "+bitexact",
               "-flags:a", "+bitexact", "-c:a", "libvorbis", "-q:a", OGG_QUALITY,
               "-ac", 1, "-ar", RATE, ogg)
        encoded[master.stem] = {"encoded_sha256": digest(ogg), "ogg_quality": OGG_QUALITY,
                                "ogg_bytes": ogg.stat().st_size}
        print(f"{ogg.name:32} {ogg.stat().st_size:8} bytes", flush=True)
    return encoded


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, help="Raw Forge WAV directory")
    parser.add_argument("--encode-only", action="store_true",
                        help="encode the masters already in scratch/audio-masters/combat")
    args = parser.parse_args()
    if args.encode_only:
        encoded = encode(MASTERS)
        report_path = DEST / "levelling.json"
        report = json.loads(report_path.read_text())
        for take, extra in encoded.items():
            report["takes"][take].update(extra)
        report_path.write_text(json.dumps(report, indent=2) + "\n")
        return
    source = args.source or PROJECT / "out/audio/sfx"
    if not source.exists():
        source = PROJECT / "assets-src/sfx/takes"
    sources = sorted(source.glob("*.wav"))
    if not sources:
        raise SystemExit(f"No rendered takes in {source}")
    MASTERS.mkdir(parents=True, exist_ok=True)
    DEST.mkdir(parents=True, exist_ok=True)
    archive = PROJECT / "assets-src/sfx/takes"
    archive.mkdir(parents=True, exist_ok=True)
    for path in sources:
        for original in (path, path.with_suffix(".json")):
            archived = archive / original.name
            if original.resolve() != archived.resolve():
                if archived.exists() and digest(archived) != digest(original):
                    raise ValueError(f"Refusing to replace a different source: {archived}")
                shutil.copyfile(original, archived)
    sources = [archive / path.name for path in sources]
    for path in sources:
        record = json.loads(path.with_suffix(".json").read_text())
        if not any(output["sha256"] == digest(path) for output in record["outputs"]):
            raise ValueError(f"Generator record does not match raw source: {path}")
    report = {"ffmpeg": ffmpeg("-version").stdout.decode().splitlines()[0],
              "script_sha256": digest(Path(__file__)), "takes": {}}
    with tempfile.TemporaryDirectory(prefix="combat-audio-") as tmp:
        for path in sources:
            event = path.stem.rsplit("_", 1)[0]
            # Compress anticipation into the AI's 650 ms charge, preserving pitch.
            prefix = "atempo=1.538461538,atrim=duration=0.65," if event == "sentry_charge" else ""
            result = render(path, MASTERS / path.name, TARGETS[event], Path(tmp), prefix)
            report["takes"][path.stem] = result
            print(f'{path.stem:24} {result["lufs"]:6.1f} LUFS  '
                  f'{result["true_peak_dbfs"]:6.1f} dBTP', flush=True)
    for take, extra in encode(MASTERS).items():
        report["takes"][take].update(extra)
    (DEST / "levelling.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
