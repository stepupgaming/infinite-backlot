"""Render Gemmy solid SOMA skins from a Kimodo NPZ.

Gemmy-owned preview renderer. It uses NVIDIA Kimodo's bundled SOMA skin mesh as
the opaque human base, then applies five channel-host materials selected during
local Gemmy content tests. It can render the default five-skin inspection sheet
or one named skin for production-oriented faceless content. This does not replace
the Kimodo NPZ; it only makes the preview artifact more useful for content
workflows.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw, ImageFilter, ImageFont

from kimodo.skeleton import SOMASkeleton77


@dataclass(frozen=True)
class Skin:
    key: str
    title: str
    body: tuple[int, int, int]
    light: tuple[int, int, int]
    dark: tuple[int, int, int]
    accent: tuple[int, int, int]
    bg: tuple[int, int, int]


SKINS = [
    Skin("obsidian_signal", "Obsidian Signal", (4, 5, 9), (58, 66, 78), (0, 0, 0), (65, 245, 255), (2, 3, 8)),
    Skin("magenta_interrupt", "Magenta Interrupt", (138, 18, 82), (255, 98, 178), (42, 4, 28), (255, 48, 132), (11, 3, 9)),
    Skin("green_data_runner", "Green Data Runner", (14, 92, 62), (88, 255, 178), (4, 26, 18), (78, 255, 166), (2, 11, 8)),
    Skin("chrome_editor", "Chrome Editor", (132, 146, 160), (255, 255, 255), (34, 40, 48), (128, 220, 255), (5, 8, 13)),
    Skin("gold_command", "Gold Command", (184, 130, 38), (255, 226, 112), (78, 45, 9), (255, 190, 66), (13, 9, 4)),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--ffmpeg", required=True)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument("--face-stride", type=int, default=1)
    parser.add_argument("--skin", choices=[skin.key for skin in SKINS])
    return parser.parse_args()


def load_font(size: int, bold: bool = False) -> ImageFont.ImageFont:
    for candidate in [
        r"C:\Windows\Fonts\segoeuib.ttf" if bold else r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arialbd.ttf" if bold else r"C:\Windows\Fonts\arial.ttf",
    ]:
        try:
            return ImageFont.truetype(candidate, size=size)
        except OSError:
            continue
    return ImageFont.load_default()


FONT_TITLE = load_font(20, True)
FONT_SMALL = load_font(12)


def load_soma_skin_class():
    path = Path(__file__).resolve().parent / "kimodo" / "viz" / "soma_skin.py"
    spec = importlib.util.spec_from_file_location("gemmy_soma_skin", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load SOMA skin module: {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.SOMASkin


def load_soma_mesh(npz_path: Path) -> tuple[np.ndarray, np.ndarray]:
    data = np.load(npz_path)
    joints = np.asarray(data["posed_joints"], dtype=np.float32)
    rots = np.asarray(data["global_rot_mats"], dtype=np.float32)
    if joints.ndim == 4:
        joints = joints[0]
    if rots.ndim == 5:
        rots = rots[0]
    skeleton = SOMASkeleton77()
    skin = load_soma_skin_class()(skeleton)
    with torch.inference_mode():
        vertices = skin.skin(
            torch.from_numpy(rots),
            torch.from_numpy(joints),
            rot_is_global=True,
        )
    return vertices.detach().cpu().numpy().astype(np.float32), skin.faces.detach().cpu().numpy().astype(np.int32)


def normalize_vertices(vertices: np.ndarray) -> np.ndarray:
    out = vertices.copy()
    center = out[:, :, [0, 2]].reshape(-1, 2).mean(axis=0)
    out[:, :, 0] -= center[0]
    out[:, :, 2] -= center[1]
    return out


def camera_project(vertices: np.ndarray, width: int, height: int) -> tuple[np.ndarray, np.ndarray]:
    x = vertices[..., 0]
    y = vertices[..., 1]
    z = vertices[..., 2]
    sx = x + 0.34 * z
    sy = y - 0.09 * z
    flat = np.stack([sx.reshape(-1), sy.reshape(-1)], axis=1)
    min_xy = flat.min(axis=0)
    max_xy = flat.max(axis=0)
    center = (min_xy + max_xy) / 2.0
    span = np.maximum(max_xy - min_xy, 1e-6)
    scale = min(width * 0.52 / span[0], height * 0.78 / span[1])
    screen = np.empty((*vertices.shape[:2], 2), dtype=np.float32)
    screen[..., 0] = (sx - center[0]) * scale + width * 0.5
    screen[..., 1] = height * 0.56 - (sy - center[1]) * scale
    depth = z - 0.24 * x
    return screen, depth


def shade(skin: Skin, intensity: float, rim: float) -> tuple[int, int, int, int]:
    intensity = max(0.0, min(1.0, intensity))
    rim = max(0.0, min(1.0, rim))
    rgb = []
    for idx in range(3):
        base = skin.dark[idx] * (1.0 - intensity) + skin.light[idx] * intensity
        base = base * (1.0 - rim) + skin.accent[idx] * rim
        rgb.append(int(max(0, min(255, base))))
    return rgb[0], rgb[1], rgb[2], 255


def draw_background(width: int, height: int, skin: Skin) -> Image.Image:
    image = Image.new("RGBA", (width, height), (*skin.bg, 255))
    draw = ImageDraw.Draw(image, "RGBA")
    for y in range(height):
        t = y / max(1, height - 1)
        draw.line((0, y, width, y), fill=(int(skin.bg[0] + 20 * t), int(skin.bg[1] + 28 * t), int(skin.bg[2] + 38 * t), 255))
    horizon = int(height * 0.8)
    for x in range(-width, width * 2, 90):
        draw.line((x, horizon, int(width * 0.5) + (x - width // 2) // 4, height), fill=(*skin.accent, 48), width=1)
    for y in range(horizon, height, 34):
        draw.line((0, y, width, y), fill=(*skin.accent, 42), width=1)
    return image


def draw_floor_shadow(image: Image.Image, points: np.ndarray) -> None:
    layer = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer, "RGBA")
    bottom = float(points[:, 1].max())
    cx = float(points[:, 0].mean())
    draw.ellipse((cx - 128, bottom - 4, cx + 128, bottom + 38), fill=(0, 0, 0, 145))
    image.alpha_composite(layer.filter(ImageFilter.GaussianBlur(13)))


def draw_mesh(image: Image.Image, skin: Skin, screen: np.ndarray, vertices: np.ndarray, faces: np.ndarray, depth: np.ndarray) -> None:
    draw_floor_shadow(image, screen)
    layer = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer, "RGBA")
    v0 = vertices[faces[:, 0]]
    v1 = vertices[faces[:, 1]]
    v2 = vertices[faces[:, 2]]
    normals = np.cross(v1 - v0, v2 - v0)
    normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-6)
    light = np.array([-0.35, 0.78, -0.52], dtype=np.float32)
    view = np.array([0.0, 0.0, -1.0], dtype=np.float32)
    lambert = np.clip(normals @ light, 0.0, 1.0)
    rim = np.power(1.0 - np.abs(np.clip(normals @ view, -1.0, 1.0)), 2.2)
    order = np.argsort(depth[faces].mean(axis=1))
    for face_idx in order:
        pts = [tuple(screen[v]) for v in faces[face_idx]]
        if max(p[0] for p in pts) < -50 or min(p[0] for p in pts) > image.width + 50:
            continue
        if max(p[1] for p in pts) < -50 or min(p[1] for p in pts) > image.height + 50:
            continue
        color = shade(skin, 0.26 + 0.74 * float(lambert[face_idx]), 0.42 * float(rim[face_idx]))
        draw.polygon(pts, fill=color)
    image.alpha_composite(layer)


def render_cell(
    skin: Skin,
    vertices: np.ndarray,
    faces: np.ndarray,
    screen: np.ndarray,
    depth: np.ndarray,
    frame: int,
    width: int,
    height: int,
    *,
    label: bool,
) -> Image.Image:
    image = draw_background(width, height, skin)
    pts = screen[frame]
    glow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(glow, "RGBA")
    draw.ellipse((pts[:, 0].min() - 34, pts[:, 1].min() - 20, pts[:, 0].max() + 34, pts[:, 1].max() + 22), fill=(*skin.accent, 34))
    image.alpha_composite(glow.filter(ImageFilter.GaussianBlur(18)))
    draw_mesh(image, skin, pts, vertices[frame], faces, depth[frame])
    draw = ImageDraw.Draw(image, "RGBA")
    if label:
        draw.rounded_rectangle((18, 16, width - 18, 56), radius=10, fill=(0, 0, 0, 150), outline=(*skin.accent, 90), width=1)
        draw.text((30, 22), skin.title.upper(), font=FONT_TITLE, fill=(242, 248, 255, 245))
        draw.text((30, height - 25), "solid SOMA skin / no wireframe", font=FONT_SMALL, fill=(210, 224, 240, 205))
    return image


def encode_frames(ffmpeg: str, frame_dir: Path, fps: int, output: Path) -> None:
    subprocess.run(
        [
            ffmpeg,
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-framerate",
            str(fps),
            "-i",
            str(frame_dir / "frame_%05d.png"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            str(output),
        ],
        check=True,
    )


def main() -> None:
    args = parse_args()
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    vertices, faces = load_soma_mesh(Path(args.input))
    vertices = normalize_vertices(vertices)
    if args.face_stride > 1:
        faces = faces[:: args.face_stride]
    selected_skins = [skin for skin in SKINS if args.skin is None or skin.key == args.skin]
    if not selected_skins:
        raise ValueError(f"unknown skin: {args.skin}")
    is_single = len(selected_skins) == 1
    base_cell_w = args.width // len(selected_skins)
    extra_pixels = args.width % len(selected_skins)
    cell_widths = [
        args.width if is_single else base_cell_w + (1 if idx < extra_pixels else 0)
        for idx in range(len(selected_skins))
    ]
    cell_h = args.height
    with tempfile.TemporaryDirectory(prefix="gemmy_kimodo_skins_") as tmp:
        frame_dir = Path(tmp)
        for frame in range(len(vertices)):
            sheet = Image.new("RGB", (args.width, cell_h), (2, 3, 6))
            x_offset = 0
            for idx, skin in enumerate(selected_skins):
                cell_w = cell_widths[idx]
                cell_screen, cell_depth = camera_project(vertices, cell_w, cell_h)
                cell = render_cell(
                    skin,
                    vertices,
                    faces,
                    cell_screen,
                    cell_depth,
                    frame,
                    cell_w,
                    cell_h,
                    label=not is_single,
                ).convert("RGB")
                sheet.paste(cell, (x_offset, 0))
                x_offset += cell_w
            sheet.save(frame_dir / f"frame_{frame:05d}.png", quality=94)
        encode_frames(args.ffmpeg, frame_dir, args.fps, output)
    manifest = {
        "status": "rendered",
        "source_npz": str(Path(args.input)),
        "output": str(output),
        "skins": [skin.__dict__ for skin in selected_skins],
        "layout": "single-skin" if is_single else "five-skins",
        "frames": int(len(vertices)),
        "fps": args.fps,
        "note": "Gemmy Kimodo preview: solid SOMA channel skin render.",
    }
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
