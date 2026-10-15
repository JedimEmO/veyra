"""Draw exported city plans without loading Bevy, Manifold, or baked meshes."""
import sys
import gzip
from pathlib import Path
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.collections import PolyCollection, LineCollection
from matplotlib.patches import Patch
import numpy as np

stem = Path(sys.argv[1])
ground_file = Path(f"{stem}-ground.tsv")
ground = np.loadtxt(ground_file if ground_file.exists() else f"{ground_file}.gz")
xs, zs = np.unique(ground[:, 0]), np.unique(ground[:, 1])
heights = ground[:, 2].reshape(len(zs), len(xs))
records = []
plan_file = Path(f"{stem}.tsv")
plan_text = plan_file.read_text() if plan_file.exists() else gzip.open(f"{plan_file}.gz", "rt").read()
for line in plan_text.splitlines():
    kind, level, yaw, *points = line.split("\t")
    records.append((kind, float(level), float(yaw), np.array([list(map(float, p.split(","))) for p in points])))

colors = {"Fittings": "#367980", "Terraces": "#c56a40", "Landing / Exchange": "#e6c57b", "Pelagic": "#354754", "Infrastructure": "#698e98"}
def district(kind):
    if kind in ("port-fittings", "exchange-dressing", "street"): return "Fittings"
    if kind in ("terrace-houses", "split-houses"): return "Terraces"
    if any(s in kind for s in ("landing", "market", "module", "exchange", "beacon")): return "Landing / Exchange"
    if any(s in kind for s in ("slab", "headquarters", "warehouse", "processor")): return "Pelagic"
    return "Infrastructure"

fig, axes = plt.subplots(1, 2, figsize=(18, 12), gridspec_kw={"width_ratios": [1.6, 1]})
fig.patch.set_facecolor("#f6f3eb")
for ax in axes:
    ax.set_facecolor("#ebe8df")
    ax.imshow(heights, extent=[xs[0], xs[-1], zs[-1], zs[0]], cmap="terrain", vmin=-25, vmax=145, alpha=.26)
    contours = ax.contour(xs, zs, heights, levels=np.arange(0, 141, 10), colors="#797f72", linewidths=.4, alpha=.65)
    ax.clabel(contours, fontsize=6, fmt="%d m")
    for category in ("terrace", "reserved", "walls", "lanes", "stair", "court"):
        polys = [p for kind, _, _, p in records if kind == category and len(p) > 2]
        if category == "terrace":
            ax.add_collection(PolyCollection(polys, facecolors="#f3eee0", edgecolors="#a39d89", linewidths=.35, alpha=.7))
        elif category == "reserved":
            ax.add_collection(PolyCollection(polys, facecolors="#ffffff", edgecolors="none", alpha=.35))
        else:
            color = "#4c514c" if category == "walls" else "#328498"
            ax.add_collection(PolyCollection(polys, facecolors=color, edgecolors="none", alpha=.85))
    turns = []
    for group, color in colors.items():
        polys = []
        for kind, level, yaw, p in records:
            if kind in ("terrace", "reserved", "walls", "lanes", "stair", "spawn", "road-exit", "court", "plant"): continue
            if district(kind) != group: continue
            polys.append(p)
            if kind in ("terrace-houses", "split-houses"):
                centre = p.mean(axis=0)
                turns.append([centre, centre + np.array([np.sin(yaw), np.cos(yaw)]) * 4])
        ax.add_collection(PolyCollection(polys, facecolors=color, edgecolors="#514d44", linewidths=.35, alpha=.9))
    ax.add_collection(LineCollection(turns, colors="#fff6da", linewidths=.5))
    for kind, level, _, p in records:
        if kind in ("spawn", "road-exit"):
            ax.scatter(*p[0], marker="*", s=125, c="#ce2546", edgecolors="white", zorder=10)
            ax.annotate(f"{kind} ({level:g} m)" if kind == "spawn" else kind, p[0], xytext=(7, 7), textcoords="offset points", fontsize=8, color="#912139")
    for kind, level, _, p in records:
        if kind == "terrace" and np.ptp(p[:, 0]) > 40:
            middle = len(p) // 4
            location = (p[middle] + p[-middle - 1]) * 0.5
            ax.text(*location, f"{level:.1f}", fontsize=4.8, color="#5e5e50", clip_on=True,
                    zorder=5, ha="center", bbox=dict(facecolor="#fffdf6", alpha=.6, pad=.2, edgecolor="none"))
    plants = np.array([p[0] for k, _, _, p in records if k == "plant"])
    if len(plants): ax.scatter(plants[:, 0], plants[:, 1], s=1.8, color="#528252", alpha=.6, zorder=4)
    ax.set_aspect("equal")
    ax.set_xlabel("Along shore, local x (m)")
    ax.set_ylabel("Downhill toward quay, local z (m)")
    ax.grid(alpha=.12)
axes[0].set(xlim=(-335, 335), ylim=(255, -480), title="Whole city | terrace edges, retaining walls, stairs and lots")
axes[1].set(xlim=(-220, 155), ylim=(130, -270), title="Exchange and neighbourhoods | white ticks face the lane")
legend = [Patch(facecolor=c, label=g) for g, c in colors.items()]
legend += [Patch(facecolor="#4c514c", label="Retaining wall"), Patch(facecolor="#328498", label="Stair / cross-lane"), Patch(facecolor="#f3eee0", edgecolor="#a39d89", label="Level bench")]
fig.legend(handles=legend, loc="lower center", ncol=4, frameon=False)
houses = sum(k in ("terrace-houses", "split-houses") for k, *_ in records)
fig.suptitle(f"PORT SEREIN / {stem.name.upper()} / {houses} terrace houses\nGround and placement from the shipping recipes; wall and landmark envelopes shown", fontsize=15)
fig.tight_layout(rect=(0, .075, 1, .94))
fig.savefig(f"{stem}.png", dpi=130, facecolor=fig.get_facecolor())
axes[0].set_visible(False)
axes[1].set_position([.08, .1, .84, .78])
axes[1].set(xlim=(-122, 0), ylim=(72, -30), title="Exchange | arrival foreground, market lanes and module edges")
fig.set_size_inches(11, 10)
fig.suptitle("PORT SEREIN / EXCHANGE ARRIVAL", fontsize=15)
fig.savefig(f"{stem}-exchange.png", dpi=130, facecolor=fig.get_facecolor())
plt.close(fig)

from collections import Counter
counts = Counter(k for k, *_ in records)
summary = "\n".join(f"{kind}: {count}" for kind, count in sorted(counts.items())) + "\n"
Path(f"{stem}-summary.txt").write_text(summary)
print(summary, end="")
