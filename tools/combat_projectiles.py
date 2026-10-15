"""Plot the authored projectile response model beside the recorded encounter.

Uses existing Matplotlib/NumPy from the ARDY environment; makes no generation
requests. The left panels are equations, the right panels are measured CSV.
"""
from pathlib import Path
import csv
import json
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "docs/design/combat/curves"
rows = list(csv.DictReader((DEST / "blaster-runtime.csv").open()))
t = np.array([float(r["seconds"]) for r in rows])
def column(name):
    return np.array([float(r[name]) for r in rows])

fig, axes = plt.subplots(2, 2, figsize=(12, 7))
fig.suptitle("Blaster revision | authored travel and measured encounter")
distance = np.linspace(0, 40, 161)
axes[0, 0].plot(distance, distance / 38, label="player: 38 m/s")
axes[0, 0].plot(distance, distance / 24, label="sentry: 24 m/s")
axes[0, 0].plot(distance, .65 + distance / 24, "--", label="sentry charge + travel")
axes[0, 0].set(xlabel="metres from muzzle", ylabel="seconds", title="Model: non-homing travel")
clock = np.linspace(0, 3, 601)
charge = np.clip(clock / .65, 0, 1)
charge[clock > .65] = 0
axes[1, 0].plot(clock, charge, label="locked-aim charge")
axes[1, 0].axvspan(.65, 2.3, alpha=.15, label="recovery before next charge")
axes[1, 0].axvline(.65+20/24, color="#da5d35", linestyle=":", label="20 m bolt arrival")
axes[1, 0].set(xlabel="seconds from charge start", ylabel="charge fraction", title="Model: sentry firing cycle")
axes[0, 1].plot(t, column("health"), label="player health")
axes[0, 1].plot(t, column("shield"), label="player shield")
axes[0, 1].plot(t, column("stamina"), alpha=.65, label="stamina")
axes[0, 1].set(xlabel="simulation seconds", ylabel="resource", title="Recorded: no immunity or damage override")
axes[1, 1].step(t, column("hostiles"), where="post", label="living hostiles")
axes[1, 1].step(t, column("bolts"), where="post", alpha=.65, label="bolts in flight")
axes[1, 1].set(xlabel="simulation seconds", ylabel="count", title="Recorded: projectiles and encounter outcome")
for ax in axes.flat:
    ax.grid(alpha=.2)
    ax.legend(fontsize=8)
fig.tight_layout()
fig.savefig(DEST / "blaster-response.png", dpi=150)
plt.close(fig)
active = np.flatnonzero(column("hostiles") > 0)
cleared = np.flatnonzero((column("hostiles") == 0) & (t > t[active[0]])) if len(active) else []
report = dict(samples=len(rows), median_tick_s=float(np.median(np.diff(t))),
              minimum_health=float(min(column("health"))), minimum_shield=float(min(column("shield"))),
              shots=int(column("shots")[-1]), enemy_shots=int(column("enemy_shots")[-1]),
              received_hits=int(column("hurts")[-1]), hostiles_remaining=int(column("hostiles")[-1]),
              cleared_at_s=float(t[cleared[0]]) if len(cleared) else None,
              authored_player_bolt_mps=38, authored_sentry_bolt_mps=24,
              authored_sentry_charge_s=.65, authored_shot_interval_s=.24)
(DEST / "blaster-response.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
