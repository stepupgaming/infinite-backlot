"""Build the editable Infinite Backlot apartment-floor set in live Blender.

Run through Blender MCP's execute_blender_code tool:
    exec(compile(open(r'C:/Projects/bevy-infinite/tools/blender/build_apartment_floor_03.py', encoding='utf-8').read(), 'build_apartment_floor_03.py', 'exec'))

The script is deterministic, uses only self-authored geometry/materials, saves the
.blend source, and creates the requested Blender reference renders. Runtime GLB
export is intentionally delegated to export_apartment_floor_03.py.
"""
from __future__ import annotations

import math
from pathlib import Path

import bpy
from mathutils import Vector

ROOT = Path(r"C:/Projects/bevy-infinite")
BLEND_PATH = ROOT / "assets/source/blender/apartment_floor_03.blend"
REFERENCE_DIR = ROOT / "assets/reference/apartment_floor_03"

COLLECTION_NAMES = (
    "SET_Static",
    "SET_Dynamic",
    "PROPS",
    "SEMANTIC_Marks",
    "SEMANTIC_Cameras",
    "COLLIDERS",
    "CUTAWAYS",
    "LIGHTING",
)


def reset_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for block in list(datablocks):
            if block.users == 0:
                datablocks.remove(block)
    for collection in list(bpy.data.collections):
        if collection.name != "Collection":
            bpy.data.collections.remove(collection)
    root = bpy.context.scene.collection
    default = bpy.data.collections.get("Collection")
    if default:
        default.name = "SET_Static"
    for name in COLLECTION_NAMES:
        if not bpy.data.collections.get(name):
            root.children.link(bpy.data.collections.new(name))


def move_to_collection(obj: bpy.types.Object, collection_name: str) -> None:
    target = bpy.data.collections[collection_name]
    for collection in list(obj.users_collection):
        collection.objects.unlink(obj)
    target.objects.link(obj)


def material(name: str, color: tuple[float, float, float, float], *, metallic: float = 0.0,
             roughness: float = 0.65, emission: tuple[float, float, float, float] | None = None,
             emission_strength: float = 0.0) -> bpy.types.Material:
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = color
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = color
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    if emission:
        bsdf.inputs["Emission Color"].default_value = emission
        bsdf.inputs["Emission Strength"].default_value = emission_strength
    mat["gltf_compatible"] = True
    return mat


def box(name: str, location: tuple[float, float, float], dimensions: tuple[float, float, float],
        mat: bpy.types.Material, collection: str = "SET_Static", bevel: float = 0.025,
        semantic_kind: str = "static") -> bpy.types.Object:
    bpy.ops.mesh.primitive_cube_add(location=location)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    obj.data.materials.append(mat)
    if bevel > 0:
        mod = obj.modifiers.new("Soft production edges", "BEVEL")
        mod.width = bevel
        mod.segments = 2
    # PROPS collection membership is itself a semantic contract. Keep callers
    # concise while ensuring every box-shaped prop appears in the sidecar.
    obj["semantic_kind"] = (
        "prop" if collection == "PROPS" and semantic_kind == "static" else semantic_kind
    )
    move_to_collection(obj, collection)
    return obj


def cylinder(name: str, location: tuple[float, float, float], radius: float, depth: float,
             mat: bpy.types.Material, collection: str = "PROPS", vertices: int = 24,
             semantic_kind: str = "prop") -> bpy.types.Object:
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=location)
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(mat)
    obj["semantic_kind"] = semantic_kind
    move_to_collection(obj, collection)
    return obj


def empty(name: str, location: tuple[float, float, float], kind: str,
          collection: str = "SEMANTIC_Marks", radius: float = 0.28) -> bpy.types.Object:
    obj = bpy.data.objects.new(name, None)
    bpy.data.collections[collection].objects.link(obj)
    obj.empty_display_type = "CIRCLE" if kind == "staging_mark" else "ARROWS"
    obj.empty_display_size = radius
    obj.location = location
    obj["semantic_kind"] = kind
    obj["semantic_id"] = name
    return obj


def camera(name: str, location: tuple[float, float, float], target: tuple[float, float, float],
           lens: float = 40.0) -> bpy.types.Object:
    data = bpy.data.cameras.new(name)
    data.lens = lens
    data.sensor_width = 36.0
    data.clip_start = 0.08
    data.clip_end = 100.0
    obj = bpy.data.objects.new(name, data)
    bpy.data.collections["SEMANTIC_Cameras"].objects.link(obj)
    obj.location = location
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat("-Z", "Y").to_euler()
    obj["semantic_kind"] = "camera_anchor"
    obj["look_at"] = list(target)
    obj["lens_mm"] = lens
    return obj


def light(name: str, location: tuple[float, float, float], color: tuple[float, float, float],
          energy: float, kind: str = "AREA", size: float = 2.0) -> bpy.types.Object:
    data = bpy.data.lights.new(name, kind)
    data.color = color
    data.energy = energy
    if kind == "AREA":
        data.shape = "RECTANGLE"
        data.size = size
        data.size_y = size * 0.45
    obj = bpy.data.objects.new(name, data)
    bpy.data.collections["LIGHTING"].objects.link(obj)
    obj.location = location
    obj.rotation_euler = (0.0, 0.0, 0.0)
    obj["semantic_kind"] = "runtime_light_reference"
    return obj


def door_frame(prefix: str, center: tuple[float, float, float], facing: str, frame_mat: bpy.types.Material,
               door_mat: bpy.types.Material, label_mat: bpy.types.Material) -> None:
    x, y, _ = center
    if facing == "west":
        box(f"SET_{prefix}_Door", (x, y, 1.12), (0.12, 1.12, 2.24), door_mat)
        box(f"SET_{prefix}_Frame_L", (x - 0.01, y - 0.64, 1.18), (0.20, 0.12, 2.42), frame_mat)
        box(f"SET_{prefix}_Frame_R", (x - 0.01, y + 0.64, 1.18), (0.20, 0.12, 2.42), frame_mat)
        box(f"SET_{prefix}_Frame_T", (x - 0.01, y, 2.34), (0.20, 1.40, 0.14), frame_mat)
        box(f"PROP_{prefix}_Number", (x - 0.075, y, 1.75), (0.025, 0.30, 0.18), label_mat, "PROPS", 0.01)
        cylinder(f"PROP_{prefix}_Handle", (x - 0.09, y - 0.36, 1.05), 0.045, 0.16, label_mat)
        bpy.context.object.rotation_euler[1] = math.radians(90)
    else:
        box(f"SET_{prefix}_Door", (x, y, 1.12), (1.12, 0.12, 2.24), door_mat)
        box(f"SET_{prefix}_Frame_L", (x - 0.64, y - 0.01, 1.18), (0.12, 0.20, 2.42), frame_mat)
        box(f"SET_{prefix}_Frame_R", (x + 0.64, y - 0.01, 1.18), (0.12, 0.20, 2.42), frame_mat)
        box(f"SET_{prefix}_Frame_T", (x, y - 0.01, 2.34), (1.40, 0.20, 0.14), frame_mat)
        box(f"PROP_{prefix}_Number", (x, y - 0.075, 1.75), (0.30, 0.025, 0.18), label_mat, "PROPS", 0.01)
        cylinder(f"PROP_{prefix}_Handle", (x - 0.36, y - 0.09, 1.05), 0.045, 0.16, label_mat)
        bpy.context.object.rotation_euler[0] = math.radians(90)


def build() -> None:
    reset_scene()
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    # Blender 5.2 exposes the realtime engine as BLENDER_EEVEE again (the
    # temporary BLENDER_EEVEE_NEXT identifier used by 4.x is no longer valid).
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.image_settings.file_format = "PNG"
    scene.render.film_transparent = False
    scene.world.color = (0.018, 0.024, 0.035)
    scene["set_id"] = "apartment_floor_03"
    scene["contract_version"] = 1
    scene["units"] = "meters"
    scene["coordinate_contract"] = "manifest coordinates are Bevy x,y,z; Blender objects use x,-z,y"

    wall = material("MAT_Wall_WarmIvory", (0.64, 0.57, 0.47, 1.0), roughness=0.82)
    accent = material("MAT_Wall_DeepTeal", (0.055, 0.18, 0.19, 1.0), roughness=0.72)
    ceiling = material("MAT_Ceiling_SoftWhite", (0.80, 0.76, 0.67, 1.0), roughness=0.90)
    terrazzo = material("MAT_Floor_Terrazzo", (0.16, 0.18, 0.18, 1.0), roughness=0.58)
    carpet = material("MAT_Carpet_Burgundy", (0.24, 0.035, 0.055, 1.0), roughness=0.98)
    trim = material("MAT_Trim_Brass", (0.47, 0.28, 0.08, 1.0), metallic=0.78, roughness=0.30)
    wood = material("MAT_Door_Walnut", (0.22, 0.075, 0.028, 1.0), roughness=0.56)
    service = material("MAT_Service_Sage", (0.12, 0.29, 0.24, 1.0), roughness=0.68)
    metal = material("MAT_Elevator_BrushedMetal", (0.31, 0.34, 0.36, 1.0), metallic=0.82, roughness=0.28)
    cabin = material("MAT_Elevator_InteriorBlue", (0.055, 0.12, 0.19, 1.0), metallic=0.36, roughness=0.34)
    black = material("MAT_Control_Black", (0.012, 0.018, 0.022, 1.0), metallic=0.35, roughness=0.25)
    glow_amber = material("MAT_Indicator_Amber", (0.55, 0.12, 0.018, 1.0), roughness=0.25,
                          emission=(1.0, 0.16, 0.02, 1.0), emission_strength=5.0)
    glow_blue = material("MAT_Panel_CoolGlow", (0.02, 0.16, 0.30, 1.0), roughness=0.22,
                         emission=(0.02, 0.42, 1.0, 1.0), emission_strength=4.0)
    prop_red = material("MAT_Prop_SafetyRed", (0.58, 0.035, 0.025, 1.0), metallic=0.1, roughness=0.48)
    plant_green = material("MAT_Prop_Plant", (0.055, 0.28, 0.10, 1.0), roughness=0.85)

    # Floors establish a broad lobby, long main hallway, and an L-turn side corridor.
    box("SET_Hallway_Main_Floor", (0.0, -2.0, -0.06), (5.2, 12.0, 0.12), terrazzo, bevel=0.01)
    box("SET_Elevator_Lobby_Floor", (-3.0, 2.8, -0.055), (10.8, 4.2, 0.11), terrazzo, bevel=0.01)
    box("SET_Hallway_Side_Floor", (5.5, -0.3, -0.05), (6.0, 4.4, 0.10), terrazzo, bevel=0.01)
    box("SET_Hallway_Main_Carpet", (0.0, -2.25, 0.015), (2.10, 10.9, 0.035), carpet, bevel=0.01)
    box("SET_Elevator_Lobby_Inlay", (-3.4, 2.75, 0.012), (9.5, 2.65, 0.025), carpet, bevel=0.01)

    # Main corridor walls, intentionally segmented to make doors and cutaways real set pieces.
    for name, loc, dims in [
        ("SET_Hallway_Main_Wall_W_A", (-2.6, -5.9, 1.55), (0.18, 4.2, 3.10)),
        ("SET_Hallway_Main_Wall_W_B", (-2.6, -0.4, 1.55), (0.18, 4.8, 3.10)),
        # The west wall stops before the lobby. Leaving this bay open creates a
        # real diagonal sightline from the main hall to the recessed elevator.
        ("SET_Hallway_Main_Wall_E_A", (2.6, -6.0, 1.55), (0.18, 4.0, 3.10)),
        ("SET_Hallway_Main_Wall_E_B", (2.6, -1.85, 1.55), (0.18, 1.7, 3.10)),
        ("SET_Hallway_Main_Wall_E_C", (2.6, 2.9, 1.55), (0.18, 2.2, 3.10)),
        ("SET_Elevator_Lobby_Wall_N_A", (-7.25, 4.90, 1.55), (2.0, 0.20, 3.10)),
        ("SET_Elevator_Lobby_Wall_N_B", (-3.25, 4.90, 1.55), (2.2, 0.20, 3.10)),
        ("SET_Elevator_Lobby_Wall_N_C", (0.50, 4.90, 1.55), (4.0, 0.20, 3.10)),
        ("SET_Elevator_Lobby_Wall_W", (-8.4, 2.8, 1.55), (0.20, 4.4, 3.10)),
        ("SET_Hallway_Side_Wall_N", (5.5, 1.90, 1.55), (6.0, 0.18, 3.10)),
        ("SET_Hallway_Side_Wall_S", (5.5, -2.50, 1.55), (6.0, 0.18, 3.10)),
        ("SET_Hallway_Side_Wall_End", (8.5, -0.3, 1.55), (0.18, 4.4, 3.10)),
    ]:
        box(name, loc, dims, wall)

    # Wainscot, baseboards, and ceiling pieces give wide shots scale and rhythm.
    for x in (-2.50, 2.50):
        box(f"SET_Hallway_Main_Wainscot_{'W' if x < 0 else 'E'}", (x, -2.0, 0.58), (0.06, 12.0, 1.16), accent, bevel=0.012)
        box(f"SET_Hallway_Main_Baseboard_{'W' if x < 0 else 'E'}", (x * 0.995, -2.0, 0.10), (0.10, 12.0, 0.20), trim, bevel=0.012)
    # Split north-wall finish around the elevator opening. A continuous strip
    # here would become a waist-high wall across the cabin threshold.
    box("SET_Elevator_Lobby_Wainscot_N_Left", (-7.45, 4.78, 0.58), (1.70, 0.06, 1.16), accent, bevel=0.012)
    box("SET_Elevator_Lobby_Wainscot_N_Right", (-1.00, 4.78, 0.58), (6.70, 0.06, 1.16), accent, bevel=0.012)
    box("SET_Elevator_Lobby_Baseboard_N_Left", (-7.45, 4.76, 0.10), (1.70, 0.10, 0.20), trim, bevel=0.012)
    box("SET_Elevator_Lobby_Baseboard_N_Right", (-1.00, 4.76, 0.10), (6.70, 0.10, 0.20), trim, bevel=0.012)
    south_cutaway = box("CUTAWAY_Hallway_South", (0.0, -8.05, 1.55), (5.2, 0.16, 3.10), wall, "CUTAWAYS", semantic_kind="cutaway")
    south_cutaway.hide_render = True
    box("CUTAWAY_Ceiling_Main", (0.0, -2.0, 3.12), (5.2, 12.0, 0.16), ceiling, "CUTAWAYS", 0.01, "cutaway")
    box("CUTAWAY_Ceiling_Lobby", (-3.0, 2.8, 3.12), (10.8, 4.2, 0.16), ceiling, "CUTAWAYS", 0.01, "cutaway")
    box("CUTAWAY_Ceiling_Side", (5.5, -0.3, 3.12), (6.0, 4.4, 0.16), ceiling, "CUTAWAYS", 0.01, "cutaway")

    # Apartment and service entrances.
    door_frame("Apartment_3B", (2.49, -5.0, 0.0), "west", trim, wood, glow_amber)
    door_frame("Apartment_3C", (2.49, -2.0, 0.0), "west", trim, wood, glow_amber)
    door_frame("Apartment_4A", (2.49, 3.0, 0.0), "west", trim, wood, glow_amber)
    door_frame("Service", (-2.49, -3.6, 0.0), "west", trim, service, glow_blue)
    door_frame("Apartment_3D", (5.5, 1.79, 0.0), "south", trim, wood, glow_amber)

    # Recessed elevator frame and a real camera-accessible cabin.
    box("SET_Elevator_Frame_Left", (-6.55, 4.72, 1.40), (0.22, 0.32, 2.80), metal)
    box("SET_Elevator_Frame_Right", (-4.45, 4.72, 1.40), (0.22, 0.32, 2.80), metal)
    box("SET_Elevator_Frame_Header", (-5.50, 4.72, 2.76), (2.32, 0.32, 0.22), metal)
    box("SET_Elevator_Threshold", (-5.50, 4.62, 0.035), (2.10, 0.45, 0.07), trim, bevel=0.01)
    box("SET_Elevator_Cabin_Floor", (-5.50, 5.75, 0.02), (2.10, 2.15, 0.08), metal, bevel=0.01)
    box("SET_Elevator_Cabin_Ceiling", (-5.50, 5.75, 2.74), (2.10, 2.15, 0.10), cabin, bevel=0.01)
    box("SET_Elevator_Cabin_Left", (-6.53, 5.75, 1.38), (0.08, 2.15, 2.72), cabin)
    box("SET_Elevator_Cabin_Right", (-4.47, 5.75, 1.38), (0.08, 2.15, 2.72), cabin)
    box("CUTAWAY_Elevator_Back", (-5.50, 6.80, 1.38), (2.10, 0.08, 2.72), cabin, "CUTAWAYS", semantic_kind="cutaway")
    box("SET_Elevator_Cabin_BackAccent", (-5.50, 6.74, 1.38), (1.55, 0.03, 2.10), accent, bevel=0.01)
    box("SET_Elevator_Cabin_Rail", (-5.50, 6.68, 1.00), (1.45, 0.07, 0.07), trim)

    left = box("DOOR_Elevator_Left", (-5.99, 4.66, 1.38), (0.98, 0.08, 2.66), metal,
               "SET_Dynamic", 0.012, "dynamic_door")
    right = box("DOOR_Elevator_Right", (-5.01, 4.66, 1.38), (0.98, 0.08, 2.66), metal,
                "SET_Dynamic", 0.012, "dynamic_door")
    left["closed_position_bevy"] = [-5.99, 1.38, -4.66]
    left["open_axis_bevy"] = [-1.0, 0.0, 0.0]
    left["travel_m"] = 0.92
    right["closed_position_bevy"] = [-5.01, 1.38, -4.66]
    right["open_axis_bevy"] = [1.0, 0.0, 0.0]
    right["travel_m"] = 0.92

    box("PROP_Floor_Indicator", (-5.50, 4.54, 2.92), (0.72, 0.09, 0.22), black, "PROPS", 0.018)
    box("PROP_Floor_Indicator_Glyph", (-5.50, 4.485, 2.92), (0.36, 0.025, 0.10), glow_amber, "SET_Dynamic", 0.008, "dynamic_indicator")
    box("PROP_Elevator_Panel", (-4.12, 4.58, 1.30), (0.30, 0.10, 0.78), black, "PROPS", 0.018)
    for idx, z in enumerate((1.55, 1.30, 1.05)):
        cylinder(f"PROP_Elevator_Panel_Button_{idx+1}", (-4.12, 4.515, z), 0.07, 0.035, glow_blue, "SET_Dynamic", 20, "interactable")
        bpy.context.object.rotation_euler[0] = math.radians(90)
    box("PROP_Call_Button", (-4.08, 4.54, 0.78), (0.22, 0.08, 0.28), black, "PROPS", 0.015)
    cylinder("PROP_Call_Button_Light", (-4.08, 4.49, 0.78), 0.065, 0.03, glow_amber, "SET_Dynamic", 20, "interactable")
    bpy.context.object.rotation_euler[0] = math.radians(90)

    # Modest set dressing: service cart, bench, plant, extinguisher, wall art.
    box("PROP_Maintenance_Cart_Body", (-1.65, -3.35, 0.48), (0.70, 1.05, 0.75), service, "PROPS", 0.04)
    box("PROP_Maintenance_Cart_Top", (-1.65, -3.35, 0.90), (0.78, 1.10, 0.08), metal, "PROPS", 0.02)
    for x in (-1.92, -1.38):
        for y in (-3.72, -2.98):
            cylinder(f"PROP_Cart_Wheel_{x}_{y}", (x, y, 0.16), 0.12, 0.06, black, "PROPS", 18)
            bpy.context.object.rotation_euler[1] = math.radians(90)
    box("PROP_Lobby_Bench_Seat", (-1.4, 3.65, 0.55), (2.1, 0.55, 0.16), wood, "PROPS", 0.05)
    box("PROP_Lobby_Bench_Back", (-1.4, 3.90, 1.05), (2.1, 0.12, 0.92), wood, "PROPS", 0.05)
    cylinder("PROP_Plant_Pot", (1.65, 3.75, 0.35), 0.34, 0.62, trim, "PROPS", 24)
    for i, (dx, dy, dz) in enumerate(((0, 0, 0.95), (.18, .06, .82), (-.15, -.05, .88), (.05, -.16, 1.05))):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=0.34, location=(1.65+dx, 3.75+dy, dz))
        obj = bpy.context.object; obj.name = f"PROP_Plant_Leaf_{i+1}"; obj.data.materials.append(plant_green); obj["semantic_kind"]="prop"; move_to_collection(obj,"PROPS")
    cylinder("PROP_Fire_Extinguisher", (-2.38, -0.8, 0.48), 0.16, 0.82, prop_red, "PROPS", 24)
    box("PROP_Wall_Art_A", (-2.48, -6.2, 1.72), (0.035, 1.10, 0.72), glow_blue, "PROPS", 0.01)
    box("PROP_Wall_Art_B", (-2.48, 0.6, 1.72), (0.035, 1.10, 0.72), glow_amber, "PROPS", 0.01)

    # Practical fixture geometry plus actual glTF-compatible lights.
    for i, y in enumerate((-6.0, -2.8, 0.4, 3.2)):
        box(f"PROP_Hall_Practical_{i+1}", (0.0 if y < 2 else -1.5, y, 3.00), (1.2, 0.34, 0.08), ceiling, "PROPS", 0.02)
        l = light(f"LIGHT_Hall_Warm_{i+1}", (0.0 if y < 2 else -1.5, y, 2.88), (1.0, 0.61, 0.34), 620.0, "AREA", 1.4)
        l.rotation_euler = (0.0, 0.0, 0.0)
    box("PROP_Elevator_Practical", (-5.5, 5.75, 2.65), (1.25, 0.55, 0.06), glow_blue, "PROPS", 0.015)
    light("LIGHT_Elevator_Cool", (-5.5, 5.75, 2.55), (0.36, 0.68, 1.0), 820.0, "AREA", 1.5)
    light("LIGHT_Lobby_Key", (-2.8, 1.9, 2.65), (1.0, 0.72, 0.48), 900.0, "AREA", 2.4)
    light("LIGHT_Side_Cool", (5.4, -0.2, 2.65), (0.38, 0.58, 1.0), 520.0, "AREA", 2.0)
    empty("LIGHTREF_Hall_Warm", (0.0, -2.8, 2.8), "lighting_reference", "LIGHTING")
    empty("LIGHTREF_Elevator_Cool", (-5.5, 5.75, 2.5), "lighting_reference", "LIGHTING")

    # Semantic staging marks are intentionally roomy for two/three-character blocking.
    marks = {
        "MARK_Hallway_TwoShot_A": (-0.85, -0.35, 0.0),
        "MARK_Hallway_TwoShot_B": (0.85, -0.35, 0.0),
        "MARK_Hallway_Group_C": (0.0, 0.70, 0.0),
        "MARK_Elevator_Threshold": (-5.50, 3.75, 0.0),
        "MARK_Elevator_Interior_A": (-5.85, 5.55, 0.0),
        "MARK_Elevator_Interior_B": (-5.15, 5.55, 0.0),
        "MARK_Panel_Interaction": (-4.10, 3.80, 0.0),
        "MARK_Hallway_Entrance": (0.0, -7.15, 0.0),
        "MARK_Apartment_3B": (1.62, -5.0, 0.0),
        "MARK_Apartment_4A": (1.62, 3.0, 0.0),
        "MARK_Service_Area": (-1.55, -2.35, 0.0),
        "MARK_Side_Corridor": (5.55, -0.30, 0.0),
    }
    for name, loc in marks.items():
        empty(name, loc, "staging_mark")

    # Camera corridors sit outside removable walls where needed; all target real set depth.
    cameras = [
        ("CAM_Hallway_Wide", (0.0, -10.8, 2.25), (0.0, -1.0, 1.25), 35.0),
        ("CAM_Hallway_Depth", (-1.75, -7.8, 1.65), (-0.2, 1.4, 1.20), 42.0),
        ("CAM_TwoShot_A", (0.0, -5.3, 1.55), (0.0, -0.2, 1.20), 48.0),
        ("CAM_OTS_Left", (-1.60, -2.7, 1.65), (0.65, -0.2, 1.30), 55.0),
        ("CAM_OTS_Right", (1.60, -2.7, 1.65), (-0.65, -0.2, 1.30), 55.0),
        # Elevator coverage stays in the open western camera corridor instead
        # of shooting diagonally through the main-hall return wall.
        ("CAM_Elevator_Reveal", (-5.50, 0.45, 1.70), (-5.50, 4.78, 1.45), 39.0),
        ("CAM_Elevator_Interior", (-5.50, 1.80, 1.60), (-5.50, 5.75, 1.30), 35.0),
        ("CAM_Panel_Insert", (-3.25, 3.15, 1.42), (-4.10, 4.55, 1.28), 62.0),
        ("CAM_Reaction_Left", (-2.8, -1.9, 1.60), (-0.85, -0.35, 1.28), 58.0),
        ("CAM_Reaction_Right", (2.8, -1.9, 1.60), (0.85, -0.35, 1.28), 58.0),
        ("CAM_Payoff", (-5.50, -0.80, 1.85), (-5.50, 4.45, 1.30), 45.0),
        ("CAM_Side_Corridor", (9.8, -0.3, 1.70), (2.2, -0.3, 1.20), 40.0),
    ]
    for spec in cameras:
        camera(*spec)

    # Explicit collision meshes: exported, hidden by the Bevy loader, and validated by name.
    for name, loc, dims in [
        ("COLLIDER_Hallway", (0.0, -2.0, 0.15), (5.0, 12.0, 0.30)),
        ("COLLIDER_Elevator", (-5.5, 5.75, 0.10), (2.0, 2.0, 0.20)),
        ("COLLIDER_Walls", (2.6, -2.0, 1.55), (0.18, 12.0, 3.10)),
    ]:
        obj = box(name, loc, dims, black, "COLLIDERS", 0.0, "collider")
        obj.display_type = "WIRE"
        obj.hide_render = True
        obj["collision_role"] = "walkable_region" if name != "COLLIDER_Walls" else "wall_boundary"

    # Deterministic scene metadata used by the exporter and review tooling.
    scene["staging_mark_count"] = len(marks)
    scene["camera_anchor_count"] = len(cameras)
    scene["dynamic_interactables"] = ["DOOR_Elevator_Left", "DOOR_Elevator_Right", "PROP_Floor_Indicator_Glyph", "PROP_Call_Button_Light"]
    scene["cutaway_groups"] = ["CUTAWAY_Hallway_South", "CUTAWAY_Ceiling_Main", "CUTAWAY_Ceiling_Lobby", "CUTAWAY_Ceiling_Side", "CUTAWAY_Elevator_Back"]

    # Freeze evaluated bevel geometry into the editable source. Re-evaluating
    # modifiers during each glTF export produced byte-level floating-point drift
    # in the BIN chunk even when the JSON topology was identical.
    bpy.ops.object.select_all(action="DESELECT")
    for obj in sorted((item for item in scene.objects if item.type == "MESH"), key=lambda item: item.name):
        bpy.context.view_layer.objects.active = obj
        obj.select_set(True)
        for modifier in list(obj.modifiers):
            bpy.ops.object.modifier_apply(modifier=modifier.name)
        obj.select_set(False)

    BLEND_PATH.parent.mkdir(parents=True, exist_ok=True)
    REFERENCE_DIR.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH), check_existing=False)

    render_specs = [
        ("hallway_wide", "CAM_Hallway_Wide", 960, 540),
        ("hallway_depth", "CAM_Hallway_Depth", 960, 540),
        ("two_character_staging", "CAM_TwoShot_A", 960, 540),
        ("elevator_exterior", "CAM_Elevator_Reveal", 960, 540),
        ("elevator_interior", "CAM_Elevator_Interior", 960, 540),
        ("panel_insert", "CAM_Panel_Insert", 960, 540),
        ("vertical_9x16", "CAM_Payoff", 540, 960),
    ]
    scene.render.resolution_percentage = 100
    for filename, camera_name, width, height in render_specs:
        # The interior reference is a doors-open target frame. Runtime motion
        # remains authored on the two named leaves; only this still hides them.
        interior_reference = filename == "elevator_interior"
        bpy.data.objects["DOOR_Elevator_Left"].hide_render = interior_reference
        bpy.data.objects["DOOR_Elevator_Right"].hide_render = interior_reference
        scene.camera = bpy.data.objects[camera_name]
        scene.render.resolution_x = width
        scene.render.resolution_y = height
        scene.render.filepath = str(REFERENCE_DIR / f"{filename}.png")
        bpy.ops.render.render(write_still=True)
    bpy.data.objects["DOOR_Elevator_Left"].hide_render = False
    bpy.data.objects["DOOR_Elevator_Right"].hide_render = False

    bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH), check_existing=False)
    print({
        "status": "built",
        "blend": str(BLEND_PATH),
        "objects": len(bpy.context.scene.objects),
        "staging_marks": len(marks),
        "camera_anchors": len(cameras),
        "references": len(render_specs),
    })


build()
