"""Gemmy bridge for Kimodo generation and lightweight motion previews.

This file is Gemmy-owned glue code. It calls the upstream Kimodo CLI module for
actual inference, then renders a default five-skin SOMA preview or a diagnostic
skeleton preview from the generated Kimodo NPZ so automated runs have a visible
artifact to inspect.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont


def load_worker_event_emitter():
    path = Path(__file__).resolve().parents[2] / "scripts" / "worker_events.py"
    spec = importlib.util.spec_from_file_location("gemmy_worker_events", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load worker event module: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.WorkerEventEmitter


WorkerEventEmitter = load_worker_event_emitter()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Gemmy Kimodo worker")
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--duration", required=True)
    parser.add_argument("--output-stem", required=True)
    parser.add_argument("--expected-npz", required=True)
    parser.add_argument("--diffusion-steps", type=int, required=True)
    parser.add_argument("--num-transition-frames", type=int, required=True)
    parser.add_argument("--num-samples", type=int, required=True)
    parser.add_argument("--constraints")
    parser.add_argument("--seed", type=int)
    parser.add_argument("--bvh", action="store_true")
    parser.add_argument("--bvh-standard-tpose", action="store_true")
    parser.add_argument("--no-postprocess", action="store_true")
    parser.add_argument("--cfg-type")
    parser.add_argument("--cfg-weight", type=float, action="append", default=[])
    parser.add_argument("--preview-output")
    parser.add_argument(
        "--preview-style",
        choices=[
            "five-skins",
            "skeleton",
            "obsidian-signal",
            "magenta-interrupt",
            "green-data-runner",
            "chrome-editor",
            "gold-command",
        ],
        default="five-skins",
    )
    parser.add_argument("--no-preview", action="store_true")
    parser.add_argument("--preview-width", type=int, default=1280)
    parser.add_argument("--preview-height", type=int, default=720)
    parser.add_argument("--preview-fps", type=int, default=30)
    parser.add_argument("--ffmpeg", required=True)
    parser.add_argument("--verbose", action="store_true")
    return parser.parse_args()


def run_upstream(args: argparse.Namespace) -> None:
    cmd = [
        sys.executable,
        "-m",
        "kimodo.scripts.generate",
        args.prompt,
        "--model",
        args.model,
        "--duration",
        args.duration,
        "--output",
        args.output_stem,
        "--num_samples",
        str(args.num_samples),
        "--diffusion_steps",
        str(args.diffusion_steps),
        "--num_transition_frames",
        str(args.num_transition_frames),
    ]
    if args.constraints:
        cmd.extend(["--constraints", args.constraints])
    if args.seed is not None:
        cmd.extend(["--seed", str(args.seed)])
    if args.bvh:
        cmd.append("--bvh")
    if args.bvh_standard_tpose:
        cmd.append("--bvh_standard_tpose")
    if args.no_postprocess:
        cmd.append("--no-postprocess")
    if args.cfg_type:
        cmd.extend(["--cfg_type", args.cfg_type])
    if args.cfg_weight:
        cmd.append("--cfg_weight")
        cmd.extend(str(weight) for weight in args.cfg_weight)
    print("[kimodo-worker] running upstream Kimodo inference")
    if args.verbose:
        print("[kimodo-worker] command:", subprocess.list2cmdline(cmd))
    sys.stdout.flush()
    subprocess.run(cmd, check=True)


def skeleton_parents(joint_count: int) -> list[int]:
    try:
        from kimodo.skeleton import SOMASkeleton30, SOMASkeleton77

        skeleton = SOMASkeleton77() if joint_count == 77 else SOMASkeleton30()
        return [int(value) for value in skeleton.joint_parents.detach().cpu().tolist()]
    except Exception as error:
        print(f"[kimodo-worker] warning: skeleton import failed for preview ({type(error).__name__}: {error})")
        return [-1] + [index - 1 for index in range(1, joint_count)]


def load_font(size: int) -> ImageFont.ImageFont:
    candidates = [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ]
    for candidate in candidates:
        try:
            return ImageFont.truetype(candidate, size=size)
        except OSError:
            continue
    return ImageFont.load_default()


def project_points(joints: np.ndarray) -> np.ndarray:
    x = joints[..., 0]
    y = joints[..., 1]
    z = joints[..., 2]
    screen_x = x + 0.32 * z
    screen_y = y - 0.12 * z
    return np.stack([screen_x, screen_y], axis=-1)


def screen_transform(projected: np.ndarray, width: int, height: int) -> tuple[np.ndarray, float]:
    flat = projected.reshape(-1, 2)
    finite = flat[np.isfinite(flat).all(axis=1)]
    if finite.size == 0:
        raise ValueError("preview cannot project motion: no finite joint positions")
    min_xy = finite.min(axis=0)
    max_xy = finite.max(axis=0)
    center = (min_xy + max_xy) / 2.0
    span = np.maximum(max_xy - min_xy, 1e-6)
    margin_x = width * 0.12
    margin_y = height * 0.14
    scale = min((width - margin_x * 2) / span[0], (height - margin_y * 2) / span[1])
    screen = np.empty_like(projected)
    screen[..., 0] = (projected[..., 0] - center[0]) * scale + width / 2.0
    screen[..., 1] = height / 2.0 - (projected[..., 1] - center[1]) * scale
    return screen, float(scale)


def draw_grid(draw: ImageDraw.ImageDraw, width: int, height: int) -> None:
    for x in range(0, width, 80):
        draw.line((x, 0, x, height), fill=(22, 35, 48), width=1)
    for y in range(0, height, 80):
        draw.line((0, y, width, y), fill=(22, 35, 48), width=1)


def root_path_screen(joints: np.ndarray, width: int, height: int) -> list[tuple[float, float]]:
    root = joints[:, 0, :]
    x = root[:, 0]
    z = root[:, 2]
    min_x, max_x = float(np.min(x)), float(np.max(x))
    min_z, max_z = float(np.min(z)), float(np.max(z))
    span_x = max(max_x - min_x, 1e-6)
    span_z = max(max_z - min_z, 1e-6)
    scale = min(220.0 / span_x, 140.0 / span_z)
    origin_x = width - 300
    origin_y = height - 110
    return [
        (origin_x + float((px - min_x) * scale), origin_y - float((pz - min_z) * scale))
        for px, pz in zip(x, z)
    ]


def render_preview(args: argparse.Namespace) -> None:
    npz_path = Path(args.expected_npz)
    preview_path = Path(args.preview_output)
    preview_path.parent.mkdir(parents=True, exist_ok=True)
    data = np.load(npz_path)
    joints = np.asarray(data["posed_joints"], dtype=np.float32)
    if joints.ndim == 4:
        joints = joints[0]
    if joints.ndim != 3 or joints.shape[-1] != 3:
        raise ValueError(f"posed_joints has unsupported shape {joints.shape}")
    frame_count, joint_count, _ = joints.shape
    parents = skeleton_parents(joint_count)
    projected = project_points(joints)
    screen, _scale = screen_transform(projected, args.preview_width, args.preview_height)
    path_points = root_path_screen(joints, args.preview_width, args.preview_height)
    title_font = load_font(24)
    small_font = load_font(16)
    with tempfile.TemporaryDirectory(prefix="gemmy_kimodo_preview_") as temp_dir:
        temp = Path(temp_dir)
        for frame_index in range(frame_count):
            image = Image.new("RGB", (args.preview_width, args.preview_height), (8, 13, 20))
            draw = ImageDraw.Draw(image)
            draw_grid(draw, args.preview_width, args.preview_height)
            if len(path_points) > 1:
                draw.line(path_points, fill=(62, 131, 255), width=3)
                px, py = path_points[frame_index]
                draw.ellipse((px - 5, py - 5, px + 5, py + 5), fill=(255, 208, 94))
                draw.text((args.preview_width - 304, args.preview_height - 278), "root path", fill=(151, 177, 210), font=small_font)
            points = screen[frame_index]
            for joint_index, parent_index in enumerate(parents[:joint_count]):
                if parent_index < 0 or parent_index >= joint_count:
                    continue
                x1, y1 = points[parent_index]
                x2, y2 = points[joint_index]
                draw.line((x1, y1, x2, y2), fill=(214, 231, 255), width=4)
                draw.line((x1, y1, x2, y2), fill=(54, 196, 255), width=1)
            for x, y in points:
                draw.ellipse((x - 3, y - 3, x + 3, y + 3), fill=(255, 255, 255))
            seconds = frame_index / max(args.preview_fps, 1)
            draw.rectangle((24, 22, args.preview_width - 24, 92), fill=(8, 13, 20))
            draw.text((36, 30), "Kimodo SOMA motion preview", fill=(237, 244, 255), font=title_font)
            draw.text(
                (36, 62),
                f"{seconds:05.2f}s  frame {frame_index + 1}/{frame_count}  prompt: {args.prompt[:110]}",
                fill=(151, 177, 210),
                font=small_font,
            )
            image.save(temp / f"frame_{frame_index:05d}.png")
        cmd = [
            args.ffmpeg,
            "-y",
            "-hide_banner",
            "-framerate",
            str(args.preview_fps),
            "-i",
            str(temp / "frame_%05d.png"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            str(preview_path),
        ]
        print("[kimodo-worker] rendering preview MP4")
        sys.stdout.flush()
        subprocess.run(cmd, check=True)


def _load_soma_skin_class():
    import importlib.util

    path = Path(__file__).resolve().parent / "kimodo" / "viz" / "soma_skin.py"
    spec = importlib.util.spec_from_file_location("gemmy_soma_skin_export", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load SOMA skin module: {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.SOMASkin


def expand_motion_to_soma77(
    joints: np.ndarray, rots: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    """Expand 30- or 77-joint Kimodo NPZ motion to SOMA 77 for Studio LBS.

    Ports the 30→77 path inside kimodo.viz.soma_skin.SOMASkin.skin so the
    browser can apply the same bind mesh + LBS weights as the NVIDIA demo.
    """
    import torch
    from kimodo.skeleton import (
        SOMASkeleton30,
        batch_rigid_transform,
        global_rots_to_local_rots,
    )

    if joints.ndim != 3 or joints.shape[-1] != 3:
        raise ValueError(f"posed_joints has unsupported shape {joints.shape}")
    if rots.ndim != 4 or rots.shape[-2:] != (3, 3):
        raise ValueError(f"global_rot_mats has unsupported shape {rots.shape}")
    if joints.shape[0] != rots.shape[0] or joints.shape[1] != rots.shape[1]:
        raise ValueError(
            f"joint/rot frame mismatch: joints={joints.shape} rots={rots.shape}"
        )

    n_frames, n_joints, _ = joints.shape
    if n_joints == 77:
        return joints.astype(np.float32), rots.astype(np.float32)
    if n_joints != 30:
        raise ValueError(f"unsupported joint count for Studio export: {n_joints}")

    skel30 = SOMASkeleton30(load=True)
    skin_cls = _load_soma_skin_class()
    skin = skin_cls(skel30)
    skel77 = skin.skeleton_skin
    with torch.inference_mode():
        jt = torch.from_numpy(np.asarray(joints, dtype=np.float32))
        rt = torch.from_numpy(np.asarray(rots, dtype=np.float32))
        local = global_rots_to_local_rots(rt, skel30)
        local77 = skel30.to_SOMASkeleton77(local)
        neutral = skel77.neutral_joints[None].repeat((n_frames, 1, 1))
        new_joint_pos, joint_rotmat = batch_rigid_transform(
            local77,
            neutral,
            skel77.joint_parents,
            skel77.root_idx,
        )
        root = jt[:, skel30.root_idx : skel30.root_idx + 1]
        joint_pos = new_joint_pos + root
    return (
        joint_pos.detach().cpu().numpy().astype(np.float32),
        joint_rotmat.detach().cpu().numpy().astype(np.float32),
    )


def export_studio_motion(npz_path: Path, fps: int = 30) -> Path:
    """Write gemmy-kimodo-motion-v1 JSON next to the NPZ for Studio WebGL LBS."""
    data = np.load(npz_path)
    joints = np.asarray(data["posed_joints"], dtype=np.float32)
    rots = np.asarray(data["global_rot_mats"], dtype=np.float32)
    if joints.ndim == 4:
        joints = joints[0]
    if rots.ndim == 5:
        rots = rots[0]
    joints77, rots77 = expand_motion_to_soma77(joints, rots)
    out_path = npz_path.with_name(f"{npz_path.stem}.studio_motion.json")
    payload = {
        "format": "gemmy-kimodo-motion-v1",
        "fps": int(fps),
        "frames": int(joints77.shape[0]),
        "joint_count": 77,
        "posed_joints": joints77.reshape(-1).tolist(),
        "global_rot_mats": rots77.reshape(-1).tolist(),
        "source_npz": str(npz_path),
    }
    out_path.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"[kimodo-worker] wrote Studio motion track {out_path}")
    sys.stdout.flush()
    return out_path


def motion_shape(npz_path: Path) -> tuple[int, int]:
    data = np.load(npz_path)
    joints = np.asarray(data["posed_joints"])
    if joints.ndim == 4:
        joints = joints[0]
    if joints.ndim != 3 or joints.shape[0] <= 0:
        raise ValueError(f"posed_joints has unsupported shape {joints.shape}")
    return int(joints.shape[0]), int(joints.shape[1])


def waypoint_count(constraints_path: str | None) -> int | None:
    if not constraints_path:
        return 0
    try:
        document = json.loads(Path(constraints_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"[observability] could not inspect Kimodo constraints: {error}", file=sys.stderr)
        return None
    constraints = document if isinstance(document, list) else [document]
    return sum(
        len(frame_indices)
        for constraint in constraints
        if isinstance(constraint, dict)
        for frame_indices in [constraint.get("frame_indices", [])]
        if isinstance(frame_indices, list)
    )


def motion_preview_observation(npz_path: Path, constraints_path: str | None) -> dict:
    frame_count, _joint_count = motion_shape(npz_path)
    return {
        "type": "motion_preview",
        "frame": 0,
        "total_frames": frame_count,
        "waypoint_count": waypoint_count(constraints_path) or 0,
    }


def main() -> None:
    args = parse_args()
    events = WorkerEventEmitter.from_environment()
    events.emit(
        "phase",
        phase="kimodo.inference",
        message="Generating SOMA motion from the motion prompt and constraints",
    )
    run_upstream(args)
    expected_npz = Path(args.expected_npz)
    if not expected_npz.exists():
        raise FileNotFoundError(f"expected Kimodo NPZ not found: {expected_npz}")

    events.emit(
        "observation",
        observation=motion_preview_observation(expected_npz, args.constraints),
    )

    # Studio Result is interactive LBS — motion track is required, not optional.
    events.emit(
        "phase",
        phase="kimodo.motion_export",
        message=f"Exporting {frame_count} motion frames for Studio LBS playback",
    )
    studio_motion_path = export_studio_motion(expected_npz, fps=int(args.preview_fps))

    if not args.no_preview and args.preview_output:
        events.emit(
            "phase",
            phase="kimodo.preview_render",
            message=f"Rendering the {args.preview_style} motion preview",
        )
        if args.preview_style == "skeleton":
            render_preview(args)
        else:
            renderer = Path(__file__).resolve().parent / "gemmy_render_kimodo_skins.py"
            cmd = [
                sys.executable,
                str(renderer),
                "--input",
                str(expected_npz),
                "--output",
                args.preview_output,
                "--ffmpeg",
                args.ffmpeg,
                "--width",
                str(args.preview_width),
                "--height",
                str(args.preview_height),
                "--fps",
                str(args.preview_fps),
            ]
            if args.preview_style != "five-skins":
                cmd.extend(["--skin", args.preview_style.replace("-", "_")])
            print(f"[kimodo-worker] rendering {args.preview_style} SOMA preview MP4")
            if args.verbose:
                print("[kimodo-worker] preview command:", subprocess.list2cmdline(cmd))
            sys.stdout.flush()
            subprocess.run(cmd, check=True)
    events.emit(
        "phase",
        phase="kimodo.complete",
        message=f"Kimodo produced {frame_count} motion frames",
    )
    print(
        json.dumps(
            {
                "status": "ok",
                "npz": str(expected_npz),
                "preview": args.preview_output if not args.no_preview else None,
                "studio_motion": str(studio_motion_path) if studio_motion_path else None,
            },
            ensure_ascii=True,
        )
    )


if __name__ == "__main__":
    main()
