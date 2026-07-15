"""Validate and deterministically export apartment_floor_03.blend to GLB + manifest.

Usage (repository root):
  "C:/Program Files/Blender Foundation/Blender 5.2/blender.exe" --background \
    assets/source/blender/apartment_floor_03.blend \
    --python tools/blender/export_apartment_floor_03.py
"""
from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
import sys

import bpy

ROOT = Path(r"C:/Projects/bevy-infinite")
GLB_PATH = ROOT / "assets/scenes/apartment_floor_03.glb"
MANIFEST_PATH = ROOT / "assets/scenes/apartment_floor_03.scene.json"

REQUIRED_NODES = {
    "SET_Hallway_Main_Floor",
    "SET_Hallway_Side_Floor",
    "SET_Elevator_Cabin_Floor",
    "SET_Elevator_Frame_Left",
    "DOOR_Elevator_Left",
    "DOOR_Elevator_Right",
    "PROP_Elevator_Panel",
    "PROP_Floor_Indicator",
    "PROP_Maintenance_Cart_Body",
    "MARK_Hallway_TwoShot_A",
    "MARK_Hallway_TwoShot_B",
    "MARK_Elevator_Threshold",
    "MARK_Elevator_Interior_A",
    "MARK_Elevator_Interior_B",
    "MARK_Panel_Interaction",
    "MARK_Hallway_Entrance",
    "CAM_Hallway_Wide",
    "CAM_Hallway_Depth",
    "CAM_TwoShot_A",
    "CAM_OTS_Left",
    "CAM_OTS_Right",
    "CAM_Elevator_Reveal",
    "CAM_Elevator_Interior",
    "CAM_Panel_Insert",
    "CAM_Reaction_Left",
    "CAM_Reaction_Right",
    "CAM_Payoff",
    "COLLIDER_Hallway",
    "COLLIDER_Elevator",
    "COLLIDER_Walls",
    "CUTAWAY_Hallway_South",
    "CUTAWAY_Elevator_Back",
}
PREFIXES = ("SET_", "DOOR_", "PROP_", "MARK_", "CAM_", "COLLIDER_", "CUTAWAY_", "LIGHT_", "LIGHTREF_")
EXPORTED_COLLECTIONS = {
    "SET_Static",
    "SET_Dynamic",
    "PROPS",
    "SEMANTIC_Marks",
    "SEMANTIC_Cameras",
    "COLLIDERS",
    "CUTAWAYS",
    "LIGHTING",
}


def fail(messages: list[str]) -> None:
    for message in messages:
        print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(2)


def blender_to_bevy(value) -> list[float]:
    return [round(float(value[0]), 6), round(float(value[2]), 6), round(-float(value[1]), 6)]


def node_record(obj: bpy.types.Object) -> dict:
    record = {
        "node": obj.name,
        "kind": obj.get("semantic_kind", "static"),
        "position": blender_to_bevy(obj.location),
    }
    if obj.type == "MESH":
        record["dimensions"] = blender_to_bevy((abs(obj.dimensions.x), abs(obj.dimensions.y), abs(obj.dimensions.z)))
        # Dimensions are extents, not directed coordinates; undo the sign used
        # by blender_to_bevy for positional Z conversion.
        record["dimensions"] = [abs(value) for value in record["dimensions"]]
    if obj.get("semantic_kind") == "cutaway":
        record["default_visible"] = not bool(obj.hide_render)
    if "collision_role" in obj:
        record["collision_role"] = obj["collision_role"]
    if obj.type == "CAMERA":
        record["look_at"] = blender_to_bevy(obj.get("look_at", [0.0, 0.0, 0.0]))
        record["lens_mm"] = round(float(obj.data.lens), 3)
    for key in ("closed_position_bevy", "open_axis_bevy", "travel_m"):
        if key in obj:
            value = obj[key]
            record[key] = list(value) if hasattr(value, "__len__") and not isinstance(value, str) else value
    return record


def validate() -> list[str]:
    errors: list[str] = []
    scene = bpy.context.scene
    if scene.unit_settings.system != "METRIC" or not math.isclose(scene.unit_settings.scale_length, 1.0, abs_tol=1e-6):
        errors.append("scene must use metric units with scale_length=1.0")
    names = {obj.name for obj in scene.objects}
    for name in sorted(REQUIRED_NODES - names):
        errors.append(f"missing required semantic node: {name}")
    for obj in scene.objects:
        if not obj.name.startswith(PREFIXES):
            errors.append(f"object has no contract prefix: {obj.name}")
        if obj.type == "MESH" and any(abs(float(scale) - 1.0) > 1e-4 for scale in obj.scale):
            errors.append(f"mesh has unapplied scale: {obj.name} scale={list(obj.scale)}")
        if obj.name.startswith("DOOR_Elevator_"):
            for prop in ("closed_position_bevy", "open_axis_bevy", "travel_m"):
                if prop not in obj:
                    errors.append(f"dynamic door {obj.name} missing {prop}")
    for collection in sorted(EXPORTED_COLLECTIONS):
        if collection not in bpy.data.collections:
            errors.append(f"missing export collection: {collection}")
    return errors


def export() -> None:
    errors = validate()
    if errors:
        fail(errors)

    scene = bpy.context.scene
    GLB_PATH.parent.mkdir(parents=True, exist_ok=True)

    bpy.ops.object.select_all(action="DESELECT")
    selected = []
    for collection_name in sorted(EXPORTED_COLLECTIONS):
        for obj in bpy.data.collections[collection_name].all_objects:
            if obj.name not in {item.name for item in selected}:
                obj.select_set(True)
                selected.append(obj)
    if not selected:
        fail(["no objects selected for export"])
    bpy.context.view_layer.objects.active = selected[0]

    result = bpy.ops.export_scene.gltf(
        filepath=str(GLB_PATH),
        check_existing=False,
        export_format="GLB",
        use_selection=True,
        use_visible=False,
        use_renderable=False,
        export_yup=True,
        # The authored .blend already contains evaluated mesh geometry. Avoid
        # modifier re-evaluation here so repeated exports are byte-identical.
        export_apply=False,
        export_cameras=True,
        export_lights=True,
        export_extras=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_animations=False,
        export_skins=False,
        export_morph=False,
        export_attributes=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_gpu_instances=False,
        export_hierarchy_full_collections=False,
        export_loglevel=-1,
        will_save_settings=False,
    )
    if "FINISHED" not in result or not GLB_PATH.exists() or GLB_PATH.stat().st_size == 0:
        fail([f"glTF exporter did not produce {GLB_PATH}: {result}"])

    objects = sorted(scene.objects, key=lambda obj: obj.name)
    by_kind: dict[str, list[dict]] = {}
    for obj in objects:
        by_kind.setdefault(obj.get("semantic_kind", "static"), []).append(node_record(obj))

    materials = []
    for mat in sorted(bpy.data.materials, key=lambda item: item.name):
        if not mat.use_nodes:
            continue
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        materials.append({
            "name": mat.name,
            "base_color": [round(float(v), 5) for v in bsdf.inputs["Base Color"].default_value],
            "metallic": round(float(bsdf.inputs["Metallic"].default_value), 5),
            "roughness": round(float(bsdf.inputs["Roughness"].default_value), 5),
            "emission_strength": round(float(bsdf.inputs["Emission Strength"].default_value), 5),
            "gltf_compatible": bool(mat.get("gltf_compatible", False)),
        })

    manifest = {
        "schema_version": 1,
        "set_id": "apartment_floor_03",
        "source_blend": "assets/source/blender/apartment_floor_03.blend",
        "runtime_glb": "assets/scenes/apartment_floor_03.glb",
        "coordinate_system": "Bevy right-handed Y-up; positions converted from Blender X/Z/-Y",
        "units": "meters",
        "required_nodes": sorted(REQUIRED_NODES),
        "static_geometry": [r for r in by_kind.get("static", []) if r["node"].startswith("SET_")],
        "dynamic_objects": sorted(by_kind.get("dynamic_door", []) + by_kind.get("dynamic_indicator", []), key=lambda r: r["node"]),
        "interactables": sorted(by_kind.get("interactable", []), key=lambda r: r["node"]),
        "props": sorted(by_kind.get("prop", []), key=lambda r: r["node"]),
        "staging_marks": sorted(by_kind.get("staging_mark", []), key=lambda r: r["node"]),
        "camera_anchors": sorted(by_kind.get("camera_anchor", []), key=lambda r: r["node"]),
        "colliders": sorted(by_kind.get("collider", []), key=lambda r: r["node"]),
        "cutaways": sorted(by_kind.get("cutaway", []), key=lambda r: r["node"]),
        "lighting_references": sorted(by_kind.get("runtime_light_reference", []) + by_kind.get("lighting_reference", []), key=lambda r: r["node"]),
        "materials": materials,
        "counts": {
            "objects": len(objects),
            "staging_marks": len(by_kind.get("staging_mark", [])),
            "camera_anchors": len(by_kind.get("camera_anchor", [])),
            "dynamic_objects": len(by_kind.get("dynamic_door", [])) + len(by_kind.get("dynamic_indicator", [])),
            "interactables": len(by_kind.get("interactable", [])),
            "colliders": len(by_kind.get("collider", [])),
            "cutaways": len(by_kind.get("cutaway", [])),
            "materials": len(materials),
        },
    }
    glb_sha = hashlib.sha256(GLB_PATH.read_bytes()).hexdigest()
    manifest["glb_sha256"] = glb_sha
    MANIFEST_PATH.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": "exported",
        "glb": str(GLB_PATH),
        "glb_bytes": GLB_PATH.stat().st_size,
        "glb_sha256": glb_sha,
        "manifest": str(MANIFEST_PATH),
        "counts": manifest["counts"],
    }, indent=2))


export()
