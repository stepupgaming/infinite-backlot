"""One-load rich Kimodo batch authoring backend for Infinite Backlot.

The worker accepts backend-neutral MotionAuthoringRequest JSON, translates safe
root paths and proxy poses to Kimodo constraints, generates bounded candidates,
scores structural errors, and persists every original candidate plus provenance.
"""
from __future__ import annotations

import argparse
import copy
import json
import math
import os
import shutil
import time
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np
import torch

from kimodo import load_model
from kimodo.constraints import (
    EndEffectorConstraintSet,
    FullBodyConstraintSet,
    Root2DConstraintSet,
    compute_global_heading,
)
from kimodo.exports.bvh import save_motion_bvh
from kimodo.exports.motion_io import save_kimodo_npz
from kimodo.geometry import quaternion_to_matrix
from kimodo.skeleton import SOMASkeleton30, global_rots_to_local_rots
from kimodo.tools import seed_everything


REFERENCE_MOTIONS = {
    "official_soma_fullbody": "kimodo/assets/demo/examples/kimodo-soma-rp/03_full_body_keyframes/motion.npz",
    "official_soma_ee": "kimodo/assets/demo/examples/kimodo-soma-rp/04_ee_constraint/motion.npz",
    "official_soma_root_path": "kimodo/assets/demo/examples/kimodo-soma-rp/05_root_path/motion.npz",
    "official_soma_waypoints": "kimodo/assets/demo/examples/kimodo-soma-rp/06_root_waypoints/motion.npz",
    "official_soma_mixed": "kimodo/assets/demo/examples/kimodo-soma-rp/07_mixed_constraints/motion.npz",
}


def emit(phase: str, **fields):
    print(json.dumps({"phase": phase, **fields}, separators=(",", ":")), flush=True)


@dataclass
class CandidateMetrics:
    root_path_deviation: float
    hand_target_error: float
    hand_orientation_error_deg: float
    foot_slide: float
    floor_penetration: float
    body_obstacle_intersections: int
    duration_error: float
    arrival_heading_error_deg: float
    contact_timing_error: float
    joint_limit_violations: int


def prompt_inputs(request: dict, fps: float) -> tuple[list[str], list[int]]:
    segments = request.get("prompt_sequence") or []
    if not segments:
        prompt = request.get("prompt")
        if not prompt:
            raise ValueError("request needs prompt_sequence or prompt")
        return [str(prompt)], [max(2, round(float(request["duration"]) * fps))]
    prompts = [str(item["text"]) for item in segments]
    frames = [max(1, round((float(item["end"]) - float(item["start"])) * fps)) for item in segments]
    target = max(2, round(float(request["duration"]) * fps))
    frames[-1] += target - sum(frames)
    return prompts, frames


def root_constraint_dict(request: dict, fps: float) -> dict | None:
    samples = request.get("dense_root_path") or request.get("root_waypoints") or []
    if not samples:
        return None
    total_frames = max(2, round(float(request["duration"]) * fps))
    indices, positions, headings = [], [], []
    for sample in samples:
        frame = min(total_frames - 1, max(0, round(float(sample["time"]) * fps)))
        if indices and frame == indices[-1]:
            positions[-1] = [float(sample["position"][0]), float(sample["position"][2])]
            headings[-1] = normalized_heading(sample.get("heading", [0.0, 0.0, 1.0]))
            continue
        indices.append(frame)
        positions.append([float(sample["position"][0]), float(sample["position"][2])])
        headings.append(normalized_heading(sample.get("heading", [0.0, 0.0, 1.0])))
    return {
        "type": "root2d",
        "frame_indices": indices,
        "smooth_root_2d": positions,
        "global_root_heading": headings,
    }


def normalized_heading(value) -> list[float]:
    x, z = float(value[0]), float(value[2])
    length = max(1e-8, math.hypot(x, z))
    # Kimodo stores heading as [cos(theta), sin(theta)] where a world-space
    # forward vector is [sin(theta), 0, cos(theta)]. Convert Backlot [x,y,z]
    # accordingly rather than treating the encoded pair as [x,z].
    return [z / length, x / length]


def resolve_reference(value: str) -> Path:
    mapped = REFERENCE_MOTIONS.get(value, value)
    path = Path(mapped)
    if not path.is_absolute():
        path = Path(__file__).resolve().parent / path
    if not path.exists():
        raise FileNotFoundError(f"reference motion not found: {value} -> {path}")
    return path


def load_reference(value: str, frame: int) -> tuple[np.ndarray, np.ndarray]:
    with np.load(resolve_reference(value), allow_pickle=False) as data:
        positions = np.asarray(data["posed_joints"], dtype=np.float32)
        rotations = np.asarray(data["global_rot_mats"], dtype=np.float32)
    if positions.ndim == 4:
        positions = positions[0]
    if rotations.ndim == 5:
        rotations = rotations[0]
    index = min(max(0, int(frame)), len(positions) - 1)
    return positions[index].copy(), rotations[index].copy()


def xyzw_matrix(value, device: torch.device) -> torch.Tensor:
    x, y, z, w = [float(v) for v in value]
    quat_wxyz = torch.tensor([w, x, y, z], dtype=torch.float32, device=device)
    return quaternion_to_matrix(quat_wxyz)


def orient_reference_pose(positions, rotations, request: dict, time_seconds: float, skeleton, device: str):
    samples = request.get("dense_root_path") or request.get("root_waypoints") or []
    if not samples:
        return positions, rotations
    sample = min(samples, key=lambda value: abs(float(value["time"]) - time_seconds))
    desired = normalized_heading(sample.get("heading", [0.0, 0.0, 1.0]))
    positions_device = torch.from_numpy(positions[None]).to(device)
    source = compute_global_heading(positions_device, skeleton)[0].detach().cpu().numpy()
    yaw = math.atan2(float(source[1]), float(source[0])) - math.atan2(desired[1], desired[0])
    cosine, sine = math.cos(yaw), math.sin(yaw)
    rotation_y = np.asarray([[cosine, 0.0, -sine], [0.0, 1.0, 0.0], [sine, 0.0, cosine]], dtype=np.float32)
    root = positions[skeleton.root_idx].copy()
    positions = (positions - root) @ rotation_y.T + root
    rotations = np.einsum("ij,njk->nik", rotation_y, rotations)
    target = np.asarray(sample["position"], dtype=np.float32)
    positions += np.asarray([target[0] - root[0], 0.0, target[2] - root[2]], dtype=np.float32)
    return positions, rotations


def build_constraints(request: dict, skeleton, fps: float, device: str) -> list:
    constraints = []
    root = root_constraint_dict(request, fps)
    if root:
        constraints.append(Root2DConstraintSet.from_dict(skeleton, root).to(device=device))

    for item in request.get("full_body_keyframes") or []:
        positions, rotations = load_reference(item["reference_motion"], item["reference_frame"])
        if positions.shape[0] != skeleton.nbjoints:
            raise ValueError(f"full body reference has {positions.shape[0]} joints; expected {skeleton.nbjoints}")
        positions, rotations = orient_reference_pose(positions, rotations, request, float(item["time"]), skeleton, device)
        target = np.asarray(item["target_root"], dtype=np.float32)
        positions += target - positions[skeleton.root_idx]
        frame = min(max(0, round(float(item["time"]) * fps)), max(1, round(float(request["duration"]) * fps)) - 1)
        constraints.append(
            FullBodyConstraintSet(
                skeleton,
                torch.tensor([frame]),
                torch.from_numpy(positions[None]).to(device),
                torch.from_numpy(rotations[None]).to(device),
                smooth_root_2d=torch.tensor([[target[0], target[2]]], device=device),
            )
        )

    grouped: dict[tuple[int, str, int], list[dict]] = {}
    for item in request.get("end_effector_constraints") or []:
        key = (round(float(item["time"]) * fps), item["reference_motion"], int(item["reference_frame"]))
        grouped.setdefault(key, []).append(item)
    for (frame, reference, reference_frame), items in grouped.items():
        positions, rotations = load_reference(reference, reference_frame)
        if positions.shape[0] != skeleton.nbjoints:
            raise ValueError(f"end-effector reference has {positions.shape[0]} joints; expected {skeleton.nbjoints}")
        positions, rotations = orient_reference_pose(positions, rotations, request, frame / fps, skeleton, device)
        names = []
        # The entire proxy pose is first oriented and translated to the validated
        # route. Exact object-relative targets then replace only selected joints.
        for item in items:
            joint = item["joint"]
            index = skeleton.bone_index[joint]
            positions[index] = np.asarray(item["position"], dtype=np.float32)
            rotations[index] = xyzw_matrix(item["rotation_xyzw"], torch.device(device)).cpu().numpy()
            names.append(joint)
        target_path = target_root_for_frames(request, max(2, round(float(request["duration"]) * fps)))
        target_frame = min(frame, len(target_path) - 1)
        smooth = target_path[target_frame][None]
        constraints.append(
            EndEffectorConstraintSet(
                skeleton,
                torch.tensor([min(frame, max(1, round(float(request["duration"]) * fps)) - 1)]),
                torch.from_numpy(positions[None]).to(device),
                torch.from_numpy(rotations[None]).to(device),
                torch.from_numpy(smooth).to(device),
                joint_names=names,
            )
        )
    # Normalize all sparse indices and data tensors onto one device. Kimodo's
    # constraint constructors deliberately leave some frame-index tensors on CPU,
    # but mixed full-body/end-effector lists are concatenated before any implicit
    # coercion inside the motion representation.
    for constraint in constraints:
        constraint.to(device=device)
        # Be explicit for upstream constraint subclasses whose .to() coverage can
        # differ: every tensor participating in create_pairs/torch.cat must share
        # the model device before Kimodo builds its condition dictionaries.
        for attribute, value in vars(constraint).items():
            if torch.is_tensor(value):
                setattr(constraint, attribute, value.to(device=device))
    return constraints


def target_root_for_frames(request: dict, frames: int) -> np.ndarray:
    samples = request.get("dense_root_path") or request.get("root_waypoints") or []
    if not samples:
        return np.zeros((frames, 2), dtype=np.float32)
    times = np.asarray([float(item["time"]) for item in samples], dtype=np.float32)
    values = np.asarray([[float(item["position"][0]), float(item["position"][2])] for item in samples], dtype=np.float32)
    query = np.linspace(0.0, float(request["duration"]), frames, dtype=np.float32)
    return np.stack([np.interp(query, times, values[:, axis]) for axis in range(2)], axis=1).astype(np.float32)


def root_path_deviation(generated: np.ndarray, target: np.ndarray) -> float:
    if len(generated) != len(target):
        indices = np.linspace(0, len(generated) - 1, len(target))
        generated = np.stack([np.interp(indices, np.arange(len(generated)), generated[:, axis]) for axis in range(2)], axis=1)
    return float(np.linalg.norm(generated - target, axis=1).max(initial=0.0))


def angle_error_degrees(a: np.ndarray, b: np.ndarray) -> float:
    a = a / max(1e-8, float(np.linalg.norm(a)))
    b = b / max(1e-8, float(np.linalg.norm(b)))
    return float(np.degrees(np.arccos(np.clip(float(np.dot(a, b)), -1.0, 1.0))))


def matrix_angle_degrees(a: np.ndarray, b: np.ndarray) -> float:
    relative = a.T @ b
    return float(np.degrees(np.arccos(np.clip((np.trace(relative) - 1.0) * 0.5, -1.0, 1.0))))


def evaluate_metrics(metrics: CandidateMetrics) -> dict:
    reasons = []
    if metrics.body_obstacle_intersections > 0:
        reasons.append("body_obstacle_intersection")
    if metrics.floor_penetration > 0.03:
        reasons.append("floor_penetration")
    if metrics.joint_limit_violations > 0:
        reasons.append("joint_limit_violation")
    if metrics.root_path_deviation > 0.5:
        reasons.append("root_corridor_violation")
    if metrics.hand_target_error > 0.25:
        reasons.append("interaction_contact_failure")
    score = (
        metrics.root_path_deviation * 4.0
        + metrics.hand_target_error * 5.0
        + metrics.hand_orientation_error_deg / 30.0
        + metrics.foot_slide * 3.0
        + metrics.floor_penetration * 10.0
        + metrics.body_obstacle_intersections * 100.0
        + metrics.duration_error * 2.0
        + metrics.arrival_heading_error_deg / 45.0
        + metrics.contact_timing_error * 3.0
        + metrics.joint_limit_violations * 100.0
    )
    return {"valid": not reasons, "score": round(float(score), 6), "rejection_reasons": reasons, "metrics": asdict(metrics)}


def count_obstacle_intersections(root_xz: np.ndarray, request: dict) -> int:
    nav = request.get("navigation_contract")
    if not nav:
        return 0
    path = Path(nav)
    if not path.is_absolute():
        path = Path(__file__).resolve().parents[2] / path
    data = json.loads(path.read_text(encoding="utf-8"))
    radius = float(request.get("actor_radius", data.get("actor_defaults", {}).get("capsule_radius", 0.34)))
    count = 0
    for x, z in root_xz:
        if any(
            abs(float(x) - float(c["center"][0])) <= float(c["half_extents"][0]) + radius
            and abs(float(z) - float(c["center"][2])) <= float(c["half_extents"][2]) + radius
            for c in data["colliders"]
        ):
            count += 1
    return count


def score_candidate(single: dict, request: dict, output_skeleton, fps: float) -> CandidateMetrics:
    roots = np.asarray(single["root_positions"], dtype=np.float32)
    generated_xz = roots[:, [0, 2]]
    target = target_root_for_frames(request, len(roots))
    posed = np.asarray(single["posed_joints"], dtype=np.float32)
    rotations = np.asarray(single["global_rot_mats"], dtype=np.float32)
    contacts = np.asarray(single.get("foot_contacts", []), dtype=np.bool_)

    hand_error, hand_angle = 0.0, 0.0
    contact_error = 0.0
    for item in request.get("end_effector_constraints") or []:
        frame = min(len(posed) - 1, max(0, round(float(item["time"]) * fps)))
        index = output_skeleton.bone_index[item["joint"]]
        error = float(np.linalg.norm(posed[frame, index] - np.asarray(item["position"], dtype=np.float32)))
        target_rot = xyzw_matrix(item["rotation_xyzw"], torch.device("cpu")).numpy()
        rotation_error = matrix_angle_degrees(rotations[frame, index], target_rot)
        if "Hand" in item["joint"]:
            hand_error = max(hand_error, error)
            hand_angle = max(hand_angle, rotation_error)
        contact_error = max(contact_error, error)

    foot_indices = [output_skeleton.bone_index[name] for name in ("LeftFoot", "RightFoot")]
    foot_slide_values = []
    for foot_number, joint_index in enumerate(foot_indices):
        velocities = np.linalg.norm(np.diff(posed[:, joint_index, [0, 2]], axis=0), axis=1)
        if contacts.size:
            start = min(foot_number * 2, contacts.shape[1] - 1)
            end = min(start + 2, contacts.shape[1])
            mask = contacts[1:, start:end].any(axis=1)
            foot_slide_values.extend(velocities[mask].tolist())
    foot_slide = float(np.mean(foot_slide_values)) if foot_slide_values else 0.0
    foot_y = posed[:, foot_indices, 1]
    floor_penetration = max(0.0, -float(foot_y.min(initial=0.0)))

    desired_heading = normalized_heading(request.get("arrival_heading", [0.0, 0.0, 1.0]))
    generated_heading = np.asarray(single.get("global_root_heading", [[0.0, 1.0]]), dtype=np.float32)[-1]
    return CandidateMetrics(
        root_path_deviation=root_path_deviation(generated_xz, target),
        hand_target_error=hand_error,
        hand_orientation_error_deg=hand_angle,
        foot_slide=foot_slide,
        floor_penetration=floor_penetration,
        body_obstacle_intersections=count_obstacle_intersections(generated_xz, request),
        duration_error=abs(len(roots) / fps - float(request["duration"])),
        arrival_heading_error_deg=angle_error_degrees(generated_heading, np.asarray(desired_heading, dtype=np.float32)),
        contact_timing_error=contact_error,
        joint_limit_violations=0,
    )


def normalize_text_export(path: Path) -> None:
    lines = path.read_text(encoding="utf-8").splitlines()
    path.write_text("\n".join(line.rstrip() for line in lines) + "\n", encoding="utf-8")


def write_motion(stem: Path, single: dict, output_skeleton, fps: float) -> tuple[Path, Path, Path]:
    stem.parent.mkdir(parents=True, exist_ok=True)
    npz_path = stem.with_suffix(".npz")
    bvh_path = stem.with_suffix(".bvh")
    sidecar_path = stem.with_suffix(".motion.json")
    save_kimodo_npz(str(npz_path), single)
    joints = torch.from_numpy(np.asarray(single["posed_joints"]))
    rotations = torch.from_numpy(np.asarray(single["global_rot_mats"]))
    local = global_rots_to_local_rots(rotations, output_skeleton)
    root = joints[:, output_skeleton.root_idx, :]
    save_motion_bvh(str(bvh_path), local, root, skeleton=output_skeleton, fps=fps, standard_tpose=True)
    normalize_text_export(bvh_path)
    sidecar_path.write_text(
        json.dumps(
            {
                "schema_version": 2,
                "sample_rate": fps,
                "root_positions": np.asarray(single["root_positions"]).tolist(),
                "global_root_heading": np.asarray(single.get("global_root_heading", [])).tolist(),
                "foot_contacts": np.asarray(single.get("foot_contacts", [])).astype(bool).tolist(),
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )
    return npz_path, bvh_path, sidecar_path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", required=True)
    parser.add_argument("--requests", required=True)
    parser.add_argument("--responses", required=True)
    parser.add_argument("--diffusion-steps", type=int, default=18)
    args = parser.parse_args()
    requests = json.loads(Path(args.requests).read_text(encoding="utf-8"))
    if isinstance(requests, dict):
        requests = [requests]
    checkpoint = Path(args.checkpoint).resolve()
    if not checkpoint.is_dir():
        raise FileNotFoundError(checkpoint)
    os.environ["CHECKPOINT_DIR"] = str(checkpoint.parent)
    os.environ.setdefault("LOCAL_CACHE", "true")
    os.environ.setdefault("TEXT_ENCODER_MODE", "local")
    os.environ.setdefault("HUGGINGFACE_CACHE_DIR", r"F:\Models\huggingface\hub")
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    emit("kimodo.model_load.started", device=device, model=checkpoint.name)
    loaded = time.perf_counter()
    model, resolved = load_model(checkpoint.name, device=device, default_family="Kimodo", return_resolved_name=True)
    emit("kimodo.model_load.completed", elapsed_ms=round((time.perf_counter() - loaded) * 1000), resolved_model=resolved)
    model_skeleton = model.skeleton
    output_skeleton = model_skeleton.somaskel77.to("cpu") if isinstance(model_skeleton, SOMASkeleton30) else model_skeleton.to("cpu")
    responses = []
    for request_index, request in enumerate(requests):
        started = time.perf_counter()
        prompts, frame_counts = prompt_inputs(request, float(model.fps))
        constraints = build_constraints(request, model_skeleton, float(model.fps), device)
        candidate_count = min(4, max(1, int(request.get("candidate_count", 1))))
        base_seed = int(request.get("seed", 0))
        base_stem = Path(request["output_stem"])
        candidates = []
        for candidate_index in range(candidate_count):
            seed = base_seed + candidate_index
            seed_everything(seed)
            emit("kimodo.inference.started", request=request["request_id"], candidate=candidate_index, seed=seed, frames=sum(frame_counts), constraints=len(constraints))
            output = model(
                prompts,
                frame_counts,
                constraint_lst=constraints,
                num_denoising_steps=args.diffusion_steps,
                num_samples=1,
                multi_prompt=True,
                num_transition_frames=10,
                post_processing=True,
                return_numpy=True,
                cfg_type="separated",
                cfg_weight=[2.0, 3.0],
            )
            single = {key: (value[0] if hasattr(value, "shape") and len(value.shape) > 0 and value.shape[0] == 1 else value) for key, value in output.items()}
            stem = base_stem.parent / f"{base_stem.name}_{candidate_index:02d}"
            export_skeleton = copy.deepcopy(output_skeleton).to("cpu")
            npz_path, bvh_path, sidecar_path = write_motion(stem, single, export_skeleton, float(model.fps))
            metrics = score_candidate(single, request, output_skeleton, float(model.fps))
            evaluation = evaluate_metrics(metrics)
            candidate = {
                "index": candidate_index,
                "seed": seed,
                "npz": str(npz_path.resolve()),
                "bvh": str(bvh_path.resolve()),
                "motion_sidecar": str(sidecar_path.resolve()),
                "evaluation": evaluation,
            }
            candidates.append(candidate)
            emit("kimodo.candidate.completed", request=request["request_id"], **candidate)
            del output, single
            torch.cuda.empty_cache()
        valid = [candidate for candidate in candidates if candidate["evaluation"]["valid"]]
        selected = min(valid, key=lambda item: (item["evaluation"]["score"], item["seed"])) if valid else None
        for candidate in candidates:
            candidate["selected"] = selected is not None and candidate["index"] == selected["index"]
        if selected is not None:
            shutil.copy2(selected["npz"], base_stem.parent / "selected.npz")
            shutil.copy2(selected["bvh"], base_stem.parent / "selected.bvh")
            shutil.copy2(selected["motion_sidecar"], base_stem.parent / "selected.motion.json")
        score_path = base_stem.parent / "candidate_scores.json"
        manifest_path = base_stem.parent / "kimodo_response_manifest.json"
        score_path.write_text(json.dumps({"schema_version": 1, "selected_index": selected["index"] if selected else None, "candidates": candidates}, indent=2), encoding="utf-8")
        manifest = {
            "schema_version": 1,
            "request_id": request["request_id"],
            "backend": "kimodo-soma-rp-v1.1",
            "checkpoint": str(checkpoint),
            "device": device,
            "diffusion_steps": args.diffusion_steps,
            "candidate_count": candidate_count,
            "candidate_seeds": [item["seed"] for item in candidates],
            "selected_candidate": selected,
            "rejected_candidates": [item for item in candidates if selected is None or item["index"] != selected["index"]],
            "constraints_applied": {
                "root": bool(request.get("dense_root_path") or request.get("root_waypoints")),
                "full_body": len(request.get("full_body_keyframes") or []),
                "end_effectors": len(request.get("end_effector_constraints") or []),
                "environment": len(request.get("environment_constraints") or []),
                "contacts": len(request.get("contact_events") or []),
            },
            "elapsed_ms": round((time.perf_counter() - started) * 1000),
        }
        manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        responses.append({"request_id": request["request_id"], "success": selected is not None, "manifest": str(manifest_path.resolve()), "candidate_scores": str(score_path.resolve()), "selected_candidate": selected})
    response_path = Path(args.responses)
    response_path.parent.mkdir(parents=True, exist_ok=True)
    response_path.write_text(json.dumps(responses, indent=2), encoding="utf-8")
    emit("kimodo.complete", response_count=len(responses), successful=sum(1 for item in responses if item["success"]))
    return 0 if all(item["success"] for item in responses) else 2


if __name__ == "__main__":
    raise SystemExit(main())
