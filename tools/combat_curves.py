"""Measure ARDY takes and baked glTF animation without claiming subjective feel.

Run with an interpreter containing numpy and matplotlib. The installed ARDY
environment supplies both. Original 20 Hz samples are retained in CSV; plots
do not pretend that interpolation adds measured motion detail.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import struct
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from scipy.spatial.transform import Rotation, Slerp

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets-src/combat/forge/assets-src/imported"
DEST = ROOT / "docs/design/combat/curves"


def measure_takes():
    DEST.mkdir(parents=True, exist_ok=True)
    reports = []
    for path in sorted(SOURCE.glob("*.npz")):
        with np.load(path, allow_pickle=False) as take:
            joints = take["posed_joints"].astype(float)
            roots = take["root_positions"].astype(float)
            rotations = take["global_rot_mats"].astype(float)
            contacts = take["foot_contacts"]
            fps = float(take["fps"])
        time = np.arange(len(joints)) / fps
        hand = joints[:, 10]
        # The rig's authored weapon axis is hand-local +Y. This is an analysis
        # proxy on the source rig, not the fitted body's authoritative collider.
        tip = hand + np.einsum("tij,j->ti", rotations[:, 10], [0, .8, 0])
        tip_relative = tip - roots
        root_speed = np.linalg.norm(np.gradient(roots[:, [0, 2]], 1 / fps, axis=0), axis=1)
        tip_speed = np.linalg.norm(np.gradient(tip_relative, 1 / fps, axis=0), axis=1)
        tip_accel = np.linalg.norm(np.gradient(np.gradient(tip_relative, 1 / fps, axis=0), 1 / fps, axis=0), axis=1)
        feet = joints[:, [25, 26, 21, 22]]
        foot_speed = np.linalg.norm(np.diff(feet, axis=0)[:, :, [0, 2]], axis=2) * fps
        planted = contacts[1:] & contacts[:-1]
        skate = float(foot_speed[planted].mean()) if planted.any() else None
        peak = int(np.argmax(tip_speed))
        report = dict(name=path.stem, source_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                      frames=len(joints), fps=fps, duration_s=float(time[-1]),
                      peak_tip_speed_mps=float(tip_speed[peak]), peak_tip_time_s=float(time[peak]),
                      peak_tip_accel_mps2=float(tip_accel.max()),
                      planted_foot_skate_mps=skate,
                      root_net_m=float(np.linalg.norm(roots[-1, [0, 2]]-roots[0, [0, 2]])),
                      kind="measured_source_rig_with_0.8m_weapon_proxy")
        reports.append(report)
        with (DEST / f"{path.stem}.csv").open("w") as out:
            writer = csv.writer(out)
            writer.writerow(["source_s", "root_speed_mps", "tip_relative_speed_mps", "tip_relative_acceleration_mps2",
                             "pelvis_y_m", "tip_x_m", "tip_y_m", "tip_z_m"])
            writer.writerows(zip(time, root_speed, tip_speed, tip_accel, roots[:, 1], *tip_relative.T))
        fig, axes = plt.subplots(3, 1, figsize=(10, 7), sharex=True)
        fig.suptitle(f"{path.stem} | measured ARDY source at {fps:g} Hz")
        axes[0].plot(time, root_speed, label="root horizontal speed")
        axes[0].plot(time, tip_speed, label="root-relative blade-tip proxy speed")
        axes[0].set_ylabel("m/s"); axes[0].legend(loc="upper right")
        axes[1].plot(time, roots[:, 1], label="pelvis height")
        axes[1].plot(time, tip_relative[:, 1], label="tip height above root")
        axes[1].set_ylabel("metres"); axes[1].legend(loc="upper right")
        axes[2].plot(time, tip_accel, color="#d45b36", label="finite-difference tip acceleration")
        axes[2].set_ylabel("m/s²"); axes[2].set_xlabel("source seconds"); axes[2].legend()
        for ax in axes: ax.grid(alpha=.2)
        fig.tight_layout(); fig.savefig(DEST / f"{path.stem}.png", dpi=140); plt.close(fig)
    (DEST / "source-measurements.json").write_text(json.dumps(reports, indent=2) + "\n")
    for row in reports:
        print(f"{row['name']:26} tip peak {row['peak_tip_speed_mps']:.2f} m/s at {row['peak_tip_time_s']:.2f}s; planted skate {row['planted_foot_skate_mps']}")


def plot_targets():
    DEST.mkdir(parents=True, exist_ok=True)
    fig, axes = plt.subplots(3, 1, figsize=(10, 8))
    t = np.linspace(0, .6, 601)
    axes[0].plot(t, np.minimum(5.5, 38*t), label="jog start: 38 m/s²")
    axes[0].plot(t, np.maximum(0, 5.5-52*t), label="jog stop: 52 m/s²")
    axes[0].set_ylabel("motor speed (m/s)"); axes[0].legend()
    u = np.clip(t/.48, 0, 1)
    axes[1].plot(t, 3.4*(1-(1-u)**3), label="dodge displacement")
    axes[1].axvspan(.06, .24, alpha=.18, color="teal", label="invulnerable")
    axes[1].set_ylabel("metres"); axes[1].legend()
    for i, (name, start, end, duration, cancel) in enumerate([
        ("Light", .13, .22, .40, .31), ("Followup", .12, .22, .40, .31), ("Heavy", .32, .45, .85, .69)]):
        axes[2].broken_barh([(0,start),(end,duration-end)], (i-.3,.6), facecolors="#7c8c98")
        axes[2].broken_barh([(start,end-start)], (i-.3,.6), facecolors="#ce693f")
        axes[2].plot([cancel,cancel],[i-.35,i+.35],color="teal",linewidth=3)
    axes[2].set_yticks([0,1,2],["Light","Followup","Heavy"])
    axes[2].set_xlabel("gameplay seconds; orange = contact, teal marker = dodge cancel")
    fig.suptitle("Authored gameplay targets | not animation measurements")
    for ax in axes: ax.grid(alpha=.2)
    fig.tight_layout(); fig.savefig(DEST / "authored-targets.png", dpi=140); plt.close(fig)


def measure_bundle(path):
    """Evaluate the shipped rig, including fitted bone lengths, before hit export.

    glTF LINEAR quaternion interpolation is spherical. Oversampling evaluates
    that interpolation; it does not claim additional captured motion detail.
    These curves use the same hand-local endpoints as combat_art.rs.
    """
    blob = path.read_bytes()
    magic, version, size = struct.unpack_from("<III", blob)
    assert magic == 0x46546C67 and version == 2 and size == len(blob)
    offset, binary, document = 12, None, None
    while offset < size:
        length, kind = struct.unpack_from("<II", blob, offset)
        chunk = blob[offset+8:offset+8+length]
        if kind == 0x4E4F534A: document = json.loads(chunk)
        elif kind == 0x004E4942: binary = chunk
        offset += 8 + length
    assert document is not None and binary is not None

    def accessor(index):
        a = document["accessors"][index]
        v = document["bufferViews"][a["bufferView"]]
        assert "sparse" not in a
        dtype = {5126:"<f4", 5123:"<u2", 5125:"<u4"}[a["componentType"]]
        width = {"SCALAR":1, "VEC3":3, "VEC4":4, "MAT4":16}[a["type"]]
        stride = v.get("byteStride", np.dtype(dtype).itemsize * width)
        return np.ndarray((a["count"], width), dtype=dtype, buffer=binary,
                          offset=v.get("byteOffset",0)+a.get("byteOffset",0),
                          strides=(stride,np.dtype(dtype).itemsize)).astype(float)

    nodes = document["nodes"]
    hand_index = next(i for i,n in enumerate(nodes) if n.get("name") == "RightHand")
    parents = {child:i for i,n in enumerate(nodes) for child in n.get("children",[])}
    chain = [hand_index]
    while chain[-1] in parents: chain.append(parents[chain[-1]])
    reports, tracks = [], {}
    definitions = {"light":(3,.40,.13,.22), "followup":(4,.40,.12,.22), "heavy":(5,.85,.32,.45)}
    for name,(index,duration,start,end) in definitions.items():
        animation = document["animations"][index]
        channels = {}
        native_duration = 0.
        for channel in animation["channels"]:
            sampler = animation["samplers"][channel["sampler"]]
            times = accessor(sampler["input"])[:,0]
            values = accessor(sampler["output"])
            interpolation = sampler.get("interpolation", "LINEAR")
            assert interpolation == "LINEAR", interpolation
            native_duration = max(native_duration, float(times[-1]))
            channels[(channel["target"]["node"],channel["target"]["path"])] = (times,values)
        game_time = np.linspace(0,duration,round(duration*120)+1)
        hands, tips = [], []
        for t in game_time * native_duration / duration:
            matrix = np.eye(4)
            for node_index in reversed(chain):
                node = nodes[node_index]
                trs = {}
                for prop,default in [("translation",[0,0,0]),("rotation",[0,0,0,1]),("scale",[1,1,1])]:
                    key = (node_index,prop)
                    if key not in channels: trs[prop] = np.array(node.get(prop,default),float)
                    else:
                        times,values = channels[key]
                        clamped = np.clip(t,times[0],times[-1])
                        if len(times)==1: trs[prop] = values[0]
                        elif prop == "rotation": trs[prop] = Slerp(times,Rotation.from_quat(values))(clamped).as_quat()
                        else: trs[prop] = np.array([np.interp(clamped,times,values[:,i]) for i in range(values.shape[1])])
                local = np.eye(4)
                local[:3,:3] = Rotation.from_quat(trs["rotation"]).as_matrix() @ np.diag(trs["scale"])
                local[:3,3] = trs["translation"]
                if "matrix" in node:
                    assert not any((node_index,p) in channels for p in trs)
                    local = np.array(node["matrix"]).reshape(4,4).T
                matrix = matrix @ local
            hands.append((matrix @ [0,.05,.005,1])[:3])
            tips.append((matrix @ [0,.83,.005,1])[:3])
        hands,tips = np.array(hands),np.array(tips)
        dt = game_time[1]-game_time[0]
        velocity = np.gradient(tips,dt,axis=0)
        speed = np.linalg.norm(velocity,axis=1)
        accel = np.linalg.norm(np.gradient(velocity,dt,axis=0),axis=1)
        peak = int(np.argmax(speed)); contact = (game_time>=start)&(game_time<=end)
        reports.append(dict(name=name, clip=animation.get("name"), clip_duration_s=native_duration,
                            action_duration_s=duration, sampled_interpolation_hz=1/dt,
                            peak_speed_mps=float(speed[peak]), peak_time_s=float(game_time[peak]),
                            contact_peak_mps=float(speed[contact].max()),
                            contact_tip_forward_m=[float(-tips[contact,2].max()),float(-tips[contact,2].min())],
                            kind="evaluated_shipped_glTF_rig_actual_0.78m_blade"))
        with (DEST/f"baked-{name}.csv").open("w") as out:
            writer=csv.writer(out); writer.writerow(["game_s","hand_x","hand_y","hand_z","tip_x","tip_y","tip_z","tip_speed_mps","tip_accel_mps2"])
            writer.writerows(zip(game_time,*hands.T,*tips.T,speed,accel))
        tracks[name] = [dict(u=float(t/duration), hand=h.tolist(), tip=p.tolist()) for t,h,p in zip(game_time,hands,tips)]
        fig,axes=plt.subplots(3,1,figsize=(10,8),sharex=True)
        fig.suptitle(f"{name}: shipped fitted rig + rendered blade | interpolated glTF")
        axes[0].plot(game_time,speed,label="tip speed"); axes[0].set_ylabel("m/s")
        axes[1].plot(game_time,-tips[:,2],label="tip forward reach"); axes[1].plot(game_time,tips[:,1],label="tip height"); axes[1].set_ylabel("m")
        axes[2].plot(game_time,accel,label="tip acceleration"); axes[2].set_ylabel("m/s²"); axes[2].set_xlabel("gameplay seconds")
        for ax in axes: ax.axvspan(start,end,alpha=.15,color="teal"); ax.grid(alpha=.2); ax.legend()
        fig.tight_layout(); fig.savefig(DEST/f"baked-{name}.png",dpi=140); plt.close(fig)
    output = path.with_name("melee.ron")
    def vector(v): return "(" + ",".join(f"{x:.7f}" for x in v) + ")"
    lines = ["// Generated by tools/combat_curves.py from the shipped GLB.","("]
    for name,track in tracks.items():
        lines.append(f"  {name}: [")
        lines.extend(f"    (u:{s['u']:.8f},hand:{vector(s['hand'])},tip:{vector(s['tip'])})," for s in track)
        lines.append("  ],")
    lines.append(")")
    output.write_text("\n".join(lines)+"\n")
    (DEST/"baked-measurements.json").write_text(json.dumps(dict(bundle_sha256=hashlib.sha256(blob).hexdigest(),curves_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),clips=reports),indent=2)+"\n")
    print(json.dumps(reports,indent=2))


def measure_runtime(path):
    with path.open() as inp: rows = list(csv.DictReader(inp))
    if not rows: raise ValueError("empty runtime trace")
    def column(key): return np.array([float(r[key]) for r in rows])
    time=column("seconds"); dt=np.diff(time)
    position=np.column_stack([column("x"),column("z")])
    speed=np.r_[0,np.linalg.norm(np.diff(position,axis=0),axis=1)/dt]
    spans=[]; first=0
    for i in range(1,len(rows)+1):
        if i==len(rows) or rows[i]["action"] != rows[first]["action"]:
            spans.append(dict(action=rows[first]["action"],start_s=float(time[first]),
                              end_s=float(time[i-1]),samples=i-first,
                              displacement_m=float(np.linalg.norm(position[i-1]-position[first]))))
            first=i
    fig,axes=plt.subplots(3,1,figsize=(12,9),sharex=True)
    fig.suptitle("Captured fixed-tick runtime | scripted input, not a human feel verdict")
    axes[0].plot(time,speed,label="actual horizontal displacement / tick")
    axes[0].plot(time,column("motor_speed"),alpha=.6,label="motor state (action travel overrides it)")
    axes[0].set_ylabel("m/s"); axes[0].legend()
    for key in ["health","shield","stamina"]: axes[1].plot(time,column(key),label=key)
    axes[1].set_ylabel("resource points"); axes[1].legend()
    actions=["Ready","Light","Followup","Heavy","Dodge","Reload","Stagger","Dead"]
    for span in spans:
        index=actions.index(span["action"])
        axes[2].broken_barh([(span["start_s"],max(.0167,span["end_s"]-span["start_s"]))],(index-.35,.7))
    axes[2].set_yticks(range(len(actions)),actions); axes[2].set_xlabel("simulation seconds")
    for ax in axes: ax.grid(alpha=.2)
    fig.tight_layout(); fig.savefig(DEST/f"{path.stem}.png",dpi=140); plt.close(fig)
    report=dict(trace_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),samples=len(rows),
                median_step_s=float(np.median(dt)),max_horizontal_speed_mps=float(speed.max()),
                shots=int(rows[-1]["shots"]),hurts=int(rows[-1]["hurts"]),actions=spans,
                note="Elapsed simulation time; manual stepping cannot benchmark wall-clock FPS. Displacement between stored endpoints omits the entry tick.")
    (DEST/f"{path.stem}.json").write_text(json.dumps(report,indent=2)+"\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--trace", type=Path)
    args = parser.parse_args()
    measure_takes()
    plot_targets()
    if args.bundle: measure_bundle(args.bundle)
    if args.trace: measure_runtime(args.trace)
