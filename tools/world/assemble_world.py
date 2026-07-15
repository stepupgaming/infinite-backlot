"""Deterministically assemble reviewed Blender-authored world modules.

Usage:
  uv run --no-project python tools/world/assemble_world.py \
      --registry assets/world/registry.json \
      --seed 424242 \
      --output data/world/demo_world_seed_424242.json
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def validate_registry(registry: dict) -> None:
    if registry.get("schema_version") != 1:
        raise ValueError("world registry schema_version must be 1")
    modules = registry.get("modules")
    if not isinstance(modules, list) or not modules:
        raise ValueError("world registry has no modules")
    seen: set[str] = set()
    for module in modules:
        module_id = module.get("module_id")
        if not module_id or module_id in seen:
            raise ValueError(f"duplicate or missing module_id: {module_id!r}")
        seen.add(module_id)
        if not module.get("sockets"):
            raise ValueError(f"module {module_id} has no socket metadata")
        for key in ("asset", "source_blend", "bounds", "staging_marks", "camera_anchors", "collision_groups"):
            if not module.get(key):
                raise ValueError(f"module {module_id} missing {key}")


def assemble(registry: dict, seed: int) -> dict:
    validate_registry(registry)
    by_id = {module["module_id"]: module for module in registry["modules"]}
    rng = random.Random(seed)
    straight_variants = ["apartment_hall_straight_a", "apartment_hall_short_a"]
    floor_one_hall = rng.choice(straight_variants)
    floor_two_hall = straight_variants[1] if floor_one_hall == straight_variants[0] else straight_variants[0]
    floor_two_junction = rng.choice(["apartment_hall_corner_a", "apartment_hall_t_junction_a"])

    placements = [
        ("street_main", "neighborhood_street_straight_a", [0, 0, 0], 0),
        ("street_intersection", "neighborhood_intersection_a", [0, 0, 21], 0),
        ("building_exterior", "apartment_exterior_a", [-10, 0, 11], 0),
        ("building_entrance", "apartment_main_entrance_a", [-10, 0, 15], 0),
        ("ground_lobby", "apartment_lobby_a", [-10, 0, 20], 0),
        ("ground_elevator", "apartment_elevator_lobby_a", [-10, 0, 27], 0),
        ("floor_one_hall", floor_one_hall, [-10, 4, 33], 0),
        ("floor_one_doorway", "apartment_doorway_section_a", [-10, 4, 40], 0),
        ("floor_two_hall", floor_two_hall, [-2, 8, 33], 90),
        ("floor_two_junction", floor_two_junction, [4, 8, 33], 90),
        ("vertical_stair", "apartment_stairwell_a", [-17, 0, 29], 0),
        ("service_room", rng.choice(["apartment_laundry_a", "apartment_maintenance_a", "apartment_boiler_room_a"]), [-18, 0, 22], 0),
        ("service_loading", "apartment_service_loading_a", [-18, 0, 14], 0),
        ("alley", "neighborhood_alley_a", [-18, 0, 2], 0),
        ("storefront_row", "neighborhood_storefront_row_a", [12, 0, 12], 0),
        ("hero_store", "neighborhood_convenience_store_a", [12, 0, 22], 0),
        ("pocket_park", "neighborhood_pocket_park_a", [20, 0, 30], 0),
        ("courtyard", "neighborhood_courtyard_a", [-1, 0, 38], 0),
        ("skyline", "neighborhood_skyline_facades_a", [0, 0, 58], 0),
    ]
    instances = []
    for index, (role, module_id, translation, yaw) in enumerate(placements):
        module = by_id[module_id]
        instances.append({
            "instance_id": f"demo_{index:02d}_{role}",
            "role": role,
            "module_id": module_id,
            "module_version": module["version"],
            "category": module["category"],
            "transform": {"translation": translation, "yaw_degrees": yaw, "scale": [1, 1, 1]},
            "runtime_state_overrides": {},
        })

    connections = [
        ["street_main", "ROAD_NORTH", "street_intersection", "ROAD_SOUTH"],
        ["street_main", "SIDEWALK_WEST", "building_exterior", "SOCKET_EXTERIOR"],
        ["building_exterior", "SOCKET_EXTERIOR", "building_entrance", "SOCKET_EXTERIOR"],
        ["building_entrance", "SOCKET_HALL_NORTH", "ground_lobby", "SOCKET_HALL_SOUTH"],
        ["ground_lobby", "SOCKET_ELEVATOR", "ground_elevator", "SOCKET_ELEVATOR"],
        ["ground_lobby", "SOCKET_HALL_NORTH", "vertical_stair", "SOCKET_HALL_SOUTH"],
        ["ground_elevator", "SOCKET_HALL_SOUTH", "floor_one_hall", "SOCKET_HALL_SOUTH"],
        ["floor_one_hall", "SOCKET_HALL_NORTH", "floor_one_doorway", "SOCKET_HALL_SOUTH"],
        ["vertical_stair", "SOCKET_STAIRS_UP", "floor_two_hall", "SOCKET_HALL_SOUTH"],
        ["floor_two_hall", "SOCKET_HALL_NORTH", "floor_two_junction", next(item["id"] for item in by_id[floor_two_junction]["sockets"] if "SOUTH" in item["id"])],
        ["service_room", next(item["id"] for item in by_id[instances[11]["module_id"]]["sockets"]), "service_loading", "SOCKET_HALL_NORTH"],
        ["service_loading", "SOCKET_EXTERIOR", "alley", "ALLEY_NORTH"],
        ["street_intersection", "ROAD_EAST", "storefront_row", "SIDEWALK_SOUTH"],
        ["storefront_row", "BUILDING_ENTRANCE_A", "hero_store", "BUILDING_ENTRANCE_MAIN"],
        ["hero_store", "ALLEY_NORTH", "courtyard", "ALLEY_EAST"],
        ["street_intersection", "ROAD_NORTH", "pocket_park", "SIDEWALK_SOUTH"],
        ["courtyard", "SIDEWALK_NORTH", "skyline", "LOT_BACKGROUND"],
    ]
    serialized_connections = [
        {"from_role": a, "from_socket": sa, "to_role": b, "to_socket": sb}
        for a, sa, b, sb in connections
    ]
    fingerprint_payload = json.dumps(
        {"seed": seed, "placements": placements, "connections": connections},
        separators=(",", ":"),
        sort_keys=True,
    ).encode()
    return {
        "schema_version": 1,
        "world_seed": seed,
        "registry_version": registry["registry_version"],
        "layout_algorithm": "backlot_demo_socket_assembler_v1",
        "layout_fingerprint": hashlib.sha256(fingerprint_payload).hexdigest(),
        "instances": instances,
        "connections": serialized_connections,
        "notes": "Demonstration layout; vertical floor translations prove reusable floor arrangements but runtime streaming is not implemented.",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry", default="assets/world/registry.json")
    parser.add_argument("--seed", type=int, default=424242)
    parser.add_argument("--output", default="data/world/demo_world_seed_424242.json")
    args = parser.parse_args()
    registry = json.loads((ROOT / args.registry).read_text(encoding="utf-8"))
    layout = assemble(registry, args.seed)
    output = ROOT / args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(layout, indent=2), encoding="utf-8")
    print(f"assembled {len(layout['instances'])} instances / {len(layout['connections'])} connections -> {output}")
    print(f"fingerprint={layout['layout_fingerprint']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
