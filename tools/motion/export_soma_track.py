"""Export a selected native Kimodo NPZ as a reusable Bevy SOMA motion track."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--npz", required=True)
    parser.add_argument("--contract", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--source-segment", required=True)
    args = parser.parse_args()
    source = Path(args.npz).resolve()
    contract_path = Path(args.contract).resolve()
    output = Path(args.output).resolve()
    contract = json.loads(contract_path.read_text(encoding="utf-8"))
    joint_names = [joint["name"] for joint in contract["joints"]]
    with np.load(source, allow_pickle=False) as data:
        positions = np.asarray(data["posed_joints"], dtype=np.float32)
        headings = np.asarray(data.get("global_root_heading", np.zeros((len(positions), 2))), dtype=np.float32)
        contacts = np.asarray(data.get("foot_contacts", np.zeros((len(positions), 6))), dtype=np.bool_)
    if positions.ndim == 4:
        positions = positions[0]
    if positions.shape[1] != len(joint_names):
        raise ValueError(f"NPZ has {positions.shape[1]} joints but contract has {len(joint_names)}")
    fps = 30
    frames = []
    for index, joints in enumerate(positions):
        encoded = headings[index] if index < len(headings) else [1.0, 0.0]
        world_heading = [float(encoded[1]), 0.0, float(encoded[0])]
        row = contacts[index] if index < len(contacts) else np.zeros(6, dtype=np.bool_)
        frames.append({
            "time": index / fps,
            "joints": joints.tolist(),
            "root_heading": world_heading,
            "foot_contacts": [bool(row[:2].any()), bool(row[2:4].any())],
        })
    track = {
        "schema_version": 1,
        "fps": fps,
        "joint_names": joint_names,
        "frames": frames,
        "source_segments": [args.source_segment],
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(track, separators=(",", ":")), encoding="utf-8")
    print(json.dumps({"output": str(output), "frames": len(frames), "joints": len(joint_names), "source": str(source)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
