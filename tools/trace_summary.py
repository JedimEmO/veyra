#!/usr/bin/env python3
"""Per-span CPU time per frame from a `--features trace` Chrome trace.

    python3 tools/trace_summary.py trace-*.json out.json 25

reads the trace (several gigabytes: it streams it line by line), takes its
last 25 seconds, and writes {"frames": n, "ms": {span name: ms per frame}}.
A span's time is summed over every thread it ran on, so a parallel pass counts
its whole CPU cost, not its wall time. Two summaries of the same view with
and without a change, diffed, say which systems the change costs.
"""
import json, sys, re, collections
path, out, secs = sys.argv[1], sys.argv[2], float(sys.argv[3])
name_re = re.compile(r'"name":"((?:[^"\\]|\\.)*)"')
# First pass: the last timestamp.
last = 0.0
with open(path) as f:
    for line in f:
        i = line.rfind('"ts":')
        if i >= 0:
            last = max(last, float(line[i + 5:].rstrip('},\n ]')))
start = last - secs * 1e6
stacks = collections.defaultdict(list)
total = collections.Counter()
frames = 0
with open(path) as f:
    for line in f:
        if '"ph":"B"' not in line and '"ph":"E"' not in line:
            continue
        i = line.rfind('"ts":')
        ts = float(line[i + 5:].rstrip('},\n ]'))
        tid = re.search(r'"tid":(\d+)', line).group(1)
        name = name_re.search(line).group(1)
        if '"ph":"B"' in line:
            stacks[tid].append((name, ts))
        else:
            if not stacks[tid]:
                continue
            n, t0 = stacks[tid].pop()
            if ts >= start:
                total[n] += ts - max(t0, start)
                if n == "sub app: name=RenderApp":
                    frames += 1
json.dump({"frames": frames, "ms": {k: v / 1000.0 / max(frames, 1) for k, v in total.items()}}, open(out, "w"), indent=0)
print(frames, "frames")
