"""Batch Kimodo worker owned by Infinite Backlot.

The worker invokes real upstream Kimodo inference, always exports NPZ and BVH,
and emits JSONL phase events for Rust timing instrumentation.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import numpy as np


def emit(phase: str, **fields) -> None:
    print(json.dumps({"phase": phase, **fields}, separators=(",", ":")), flush=True)


def waypoint_constraints(request: dict) -> Path | None:
    waypoints = sorted(request.get("root_waypoints") or [], key=lambda item: int(item["frame"]))
    if not waypoints:
        value = request.get("constraints")
        return Path(value) if value else None
    if len({int(item["frame"]) for item in waypoints}) < 2:
        raise ValueError("root waypoint motion requires at least two distinct frames")
    path = Path(request["output_stem"]).with_suffix(".constraints.json")
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = [{
        "type": "root2d",
        "frame_indices": [int(item["frame"]) for item in waypoints],
        "smooth_root_2d": [[float(item["x"]), float(item["z"])] for item in waypoints],
    }]
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", required=True)
    parser.add_argument("--requests", required=True)
    parser.add_argument("--responses", required=True)
    parser.add_argument("--diffusion-steps", type=int, default=20)
    args = parser.parse_args()
    requests = json.loads(Path(args.requests).read_text(encoding="utf-8"))
    if not isinstance(requests, list):
        raise ValueError("request file must contain a JSON array")

    checkpoint = Path(args.checkpoint).resolve()
    if not checkpoint.is_dir():
        raise FileNotFoundError(f"Kimodo checkpoint is not a directory: {checkpoint}")
    # Upstream resolves a registered model name and locates the corresponding
    # directory below CHECKPOINT_DIR. Passing the checkpoint path as --model is
    # invalid even though the weights exist.
    os.environ["CHECKPOINT_DIR"] = str(checkpoint.parent)
    os.environ.setdefault("LOCAL_CACHE", "true")
    os.environ.setdefault("TEXT_ENCODER_MODE", "local")
    os.environ.setdefault("HUGGINGFACE_CACHE_DIR", r"F:\Models\huggingface\hub")
    model_name = checkpoint.name

    responses = []
    for index, request in enumerate(requests):
        started = time.perf_counter()
        output_stem = Path(request["output_stem"])
        output_stem.parent.mkdir(parents=True, exist_ok=True)
        constraints = waypoint_constraints(request)
        command = [
            sys.executable, "-m", "kimodo.scripts.generate", request["prompt"],
            "--model", model_name,
            "--duration", str(float(request["duration"])),
            "--output", str(output_stem),
            "--num_samples", "1",
            "--diffusion_steps", str(args.diffusion_steps),
            "--num_transition_frames", "10",
            "--seed", str(int(request.get("seed", 0))),
            "--bvh", "--bvh_standard_tpose",
        ]
        if constraints:
            command.extend(["--constraints", str(constraints)])
        emit("kimodo.inference.started", index=index, semantic=request.get("semantic"))
        subprocess.run(command, check=True)
        candidates = [output_stem.with_suffix(".npz"), Path(f"{output_stem}_0.npz")]
        npz = next((path for path in candidates if path.exists()), None)
        if npz is None:
            raise FileNotFoundError(f"Kimodo did not produce NPZ for {output_stem}")
        bvh_candidates = [output_stem.with_suffix(".bvh"), npz.with_suffix(".bvh")]
        bvh = next((path for path in bvh_candidates if path.exists()), None)
        if bvh is None:
            raise FileNotFoundError(f"Kimodo did not produce BVH for {output_stem}")
        motion = np.load(npz)
        root_positions = np.asarray(motion["root_positions"], dtype=np.float32)
        contacts = np.asarray(motion["foot_contacts"], dtype=np.bool_)
        posed_joints = np.asarray(motion["posed_joints"], dtype=np.float32)
        if root_positions.ndim != 2 or root_positions.shape[1] != 3:
            raise ValueError(f"unexpected Kimodo root_positions shape {root_positions.shape}")
        if contacts.ndim != 2:
            raise ValueError(f"unexpected Kimodo foot_contacts shape {contacts.shape}")
        contact_joint_indices = [69, 70, 71, 74, 75, 76]
        if posed_joints.ndim != 3 or posed_joints.shape[1] <= max(contact_joint_indices):
            raise ValueError(f"unexpected Kimodo posed_joints shape {posed_joints.shape}")
        foot_positions = posed_joints[:, contact_joint_indices, :]
        sidecar = output_stem.with_suffix(".motion.json")
        sidecar.write_text(json.dumps({
            "schema_version": 1,
            "sample_rate": 30.0,
            "root_positions": root_positions.tolist(),
            "foot_contacts": contacts.tolist(),
            "foot_positions": foot_positions.tolist(),
            "contact_channels": [f"contact_{i}" for i in range(contacts.shape[1])],
        }, separators=(",", ":")), encoding="utf-8")
        response = {
            "index": index,
            "semantic": request.get("semantic"),
            "npz": str(npz.resolve()),
            "bvh": str(bvh.resolve()),
            "motion_sidecar": str(sidecar.resolve()),
            "constraints": str(constraints.resolve()) if constraints else None,
            "elapsed_ms": round((time.perf_counter() - started) * 1000),
            "success": True,
        }
        responses.append(response)
        emit("kimodo.motion_export.completed", **response)

    response_path = Path(args.responses)
    response_path.parent.mkdir(parents=True, exist_ok=True)
    response_path.write_text(json.dumps(responses, indent=2), encoding="utf-8")
    emit("kimodo.complete", response_count=len(responses))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
