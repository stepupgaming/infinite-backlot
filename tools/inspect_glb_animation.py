"""Inspect sampled TRS channels in a GLB without adding Python dependencies."""

from __future__ import annotations

import argparse
import json
import math
import struct
from pathlib import Path


COMPONENT_FORMAT = {5126: "f", 5125: "I", 5123: "H", 5122: "h", 5121: "B", 5120: "b"}
COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def load_glb(path: Path) -> tuple[dict, bytes]:
    with path.open("rb") as source:
        magic, version, _length = struct.unpack("<4sII", source.read(12))
        if magic != b"glTF" or version != 2:
            raise ValueError("expected GLB v2")
        json_length, _ = struct.unpack("<II", source.read(8))
        document = json.loads(source.read(json_length).decode().rstrip("\x00 "))
        binary_length, _ = struct.unpack("<II", source.read(8))
        return document, source.read(binary_length)


def accessor_values(document: dict, binary: bytes, index: int) -> list[tuple[float, ...]]:
    accessor = document["accessors"][index]
    view = document["bufferViews"][accessor["bufferView"]]
    width = COMPONENTS[accessor["type"]]
    code = COMPONENT_FORMAT[accessor["componentType"]]
    item_size = struct.calcsize("<" + code * width)
    stride = view.get("byteStride", item_size)
    offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    return [
        struct.unpack_from("<" + code * width, binary, offset + row * stride)
        for row in range(accessor["count"])
    ]


def qmul(a: tuple[float, ...], b: tuple[float, ...]) -> tuple[float, ...]:
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def qconj(q: tuple[float, ...]) -> tuple[float, ...]:
    return (-q[0], -q[1], -q[2], q[3])


def qrotate(q: tuple[float, ...], v: tuple[float, float, float]) -> tuple[float, ...]:
    rotated = qmul(qmul(q, (v[0], v[1], v[2], 0.0)), qconj(q))
    return rotated[:3]


def qfrom_to(source: tuple[float, ...], target: tuple[float, ...]) -> tuple[float, ...]:
    sx, sy, sz = source
    tx, ty, tz = target
    dot = sx * tx + sy * ty + sz * tz
    cross = (sy * tz - sz * ty, sz * tx - sx * tz, sx * ty - sy * tx)
    q = (cross[0], cross[1], cross[2], 1.0 + dot)
    length = math.sqrt(sum(value * value for value in q))
    return tuple(value / length for value in q)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path)
    parser.add_argument("--animation", default="Idle")
    parser.add_argument("--node", action="append", default=["upperarm.l", "upperarm.r"])
    parser.add_argument("--solve-relaxed", action="store_true")
    args = parser.parse_args()
    document, binary = load_glb(args.path)
    animation = next(item for item in document["animations"] if item.get("name") == args.animation)
    parents = {
        child: parent
        for parent, node in enumerate(document["nodes"])
        for child in node.get("children", [])
    }
    wanted = set(args.node)
    for channel in animation["channels"]:
        node_index = channel["target"]["node"]
        node_name = document["nodes"][node_index].get("name", str(node_index))
        if node_name not in wanted:
            continue
        sampler = animation["samplers"][channel["sampler"]]
        times = accessor_values(document, binary, sampler["input"])
        values = accessor_values(document, binary, sampler["output"])
        samples = [(times[i][0], values[i]) for i in range(min(len(times), len(values)))]
        print(node_name, channel["target"]["path"], samples[:8])
        if args.solve_relaxed and channel["target"]["path"] == "rotation" and "upperarm" in node_name:
            chain = []
            parent = parents.get(node_index)
            while parent is not None:
                chain.append(parent)
                parent = parents.get(parent)
            parent_rotation = (0.0, 0.0, 0.0, 1.0)
            for ancestor in reversed(chain):
                parent_rotation = qmul(
                    parent_rotation,
                    tuple(document["nodes"][ancestor].get("rotation", [0.0, 0.0, 0.0, 1.0])),
                )
            local = values[min(7, len(values) - 1)]
            global_rotation = qmul(parent_rotation, local)
            current = qrotate(global_rotation, (0.0, 1.0, 0.0))
            side = 0.18 if node_name.endswith(".l") else -0.18
            target = (side, -0.98, 0.0)
            length = math.sqrt(sum(value * value for value in target))
            target = tuple(value / length for value in target)
            correction = qfrom_to(current, target)
            solved = qmul(qmul(qmul(qconj(parent_rotation), correction), parent_rotation), local)
            length = math.sqrt(sum(value * value for value in solved))
            print("solved_relaxed", tuple(value / length for value in solved), "current", current)


if __name__ == "__main__":
    main()
