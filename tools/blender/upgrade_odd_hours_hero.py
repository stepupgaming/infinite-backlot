"""Build the production Odd Hours hero set and its runtime semantic contracts.

Run inside Blender 5.2, including through Blender MCP. The result is the editable
source .blend, runtime GLB, hero preview, module sidecar, and a focused navigation
contract used by the production vertical-slice compiler.
"""
from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

import bpy
from mathutils import Vector

ROOT = Path(r"C:\Projects\bevy-infinite")
SOURCE = ROOT / "assets/source/blender/world/locations/location_odd_hours_v3.blend"
GLB = ROOT / "assets/world/locations/location_odd_hours_v3.glb"
SIDECAR = ROOT / "assets/world/locations/location_odd_hours_v3.scene.json"
NAV = ROOT / "assets/world/navigation/odd_hours_production.json"
PREVIEW = ROOT / "assets/reference/production-vertical-slice/odd_hours_hero_preview.png"
REGISTRY = ROOT / "assets/world/registry.json"

for object_ in list(bpy.data.objects):
    bpy.data.objects.remove(object_, do_unlink=True)
for collection_ in list(bpy.data.collections):
    bpy.data.collections.remove(collection_)
root_collection = bpy.data.collections.new("ODD_HOURS_HERO")
bpy.context.scene.collection.children.link(root_collection)


def collection(name: str):
    value = bpy.data.collections.get(name) or bpy.data.collections.new(name)
    if value.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(value)
    return value


ARCH = collection("ARCHITECTURE")
FIXTURES = collection("FIXTURES")
PRODUCTS = collection("PRODUCTS")
PROPS = collection("PROPS_CC0_ADAPTED")
CUTAWAY = collection("CUTAWAY_FRONT")
SEMANTICS = collection("SEMANTICS")
LIGHTS = collection("LIGHTING")


def move_to_collection(object_, target):
    for owner in list(object_.users_collection):
        owner.objects.unlink(object_)
    target.objects.link(object_)


def material(name, color, roughness=0.65, metallic=0.0, emission=None, alpha=1.0):
    value = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    value.diffuse_color = (*color, alpha)
    value.use_nodes = True
    bsdf = value.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = metallic
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = 3.0
    if alpha < 1.0:
        bsdf.inputs["Alpha"].default_value = alpha
        value.surface_render_method = "DITHERED"
    return value


MAT = {
    "floor": material("OH_FLOOR_TERRAZZO", (0.16, 0.20, 0.23), 0.48),
    "sidewalk": material("OH_SIDEWALK", (0.18, 0.19, 0.21), 0.82),
    "wall": material("OH_WALL_PLUM", (0.24, 0.055, 0.11), 0.68),
    "trim": material("OH_TRIM_NAVY", (0.025, 0.05, 0.11), 0.4, 0.08),
    "cream": material("OH_FIXTURE_CREAM", (0.72, 0.68, 0.57), 0.58),
    "counter": material("OH_COUNTER_TEAL", (0.035, 0.30, 0.32), 0.45),
    "countertop": material("OH_COUNTERTOP", (0.06, 0.08, 0.10), 0.28, 0.2),
    "shelf": material("OH_SHELF_BLUE", (0.055, 0.13, 0.22), 0.45, 0.12),
    "glass": material("OH_GLASS", (0.11, 0.28, 0.36), 0.18, alpha=0.28),
    "door": material("OH_DOOR_CORAL", (0.68, 0.12, 0.16), 0.35),
    "gold": material("OH_GOLD", (0.94, 0.55, 0.10), 0.28, 0.35),
    "neon": material("OH_NEON_MINT", (0.04, 0.80, 0.65), 0.22, emission=(0.04, 0.85, 0.68)),
    "light": material("OH_LIGHT_PANEL", (0.96, 0.86, 0.62), 0.22, emission=(1.0, 0.78, 0.42)),
    "package": material("OH_PACKAGE_PURPLE", (0.38, 0.08, 0.60), 0.5),
    "paper": material("OH_PRICE_CARD", (0.92, 0.78, 0.38), 0.7),
}
PRODUCT_MATS = [
    material("OH_PRODUCT_RED", (0.78, 0.09, 0.12), 0.6),
    material("OH_PRODUCT_ORANGE", (0.92, 0.34, 0.06), 0.6),
    material("OH_PRODUCT_MINT", (0.08, 0.63, 0.47), 0.6),
    material("OH_PRODUCT_BLUE", (0.08, 0.24, 0.64), 0.6),
    material("OH_PRODUCT_PINK", (0.72, 0.14, 0.42), 0.6),
]


def add_box(name, location, dimensions, mat, target=ARCH, bevel=0.04):
    bpy.ops.mesh.primitive_cube_add(location=location)
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel > 0:
        modifier = obj.modifiers.new("ConsistentBevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 2
    obj.data.materials.append(mat)
    move_to_collection(obj, target)
    return obj


def add_cylinder(name, location, radius, depth, mat, target=FIXTURES, vertices=16):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=location)
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.append(mat)
    move_to_collection(obj, target)
    return obj


def add_text(name, text, location, size, extrude, mat, target, rotation=(math.pi / 2, 0, 0), align="CENTER"):
    bpy.ops.object.text_add(location=location, rotation=rotation)
    obj = bpy.context.object
    obj.name = name
    obj.data.body = text
    obj.data.align_x = align
    obj.data.align_y = "CENTER"
    obj.data.size = size
    obj.data.extrude = extrude
    obj.data.bevel_depth = extrude * 0.35
    obj.data.materials.append(mat)
    move_to_collection(obj, target)
    return obj


# Architecture: store shell, exterior sidewalk, readable storefront, and entrance.
add_box("OH_INTERIOR_FLOOR", (0, 0, -0.10), (10, 9, 0.20), MAT["floor"])
add_box("OH_EXTERIOR_SIDEWALK", (0, -6.7, -0.12), (10, 4.6, 0.24), MAT["sidewalk"])
for stripe_x in (-3.6, -1.8, 0.0, 1.8, 3.6):
    add_box(f"OH_SIDEWALK_JOINT_{stripe_x:+.1f}", (stripe_x, -6.7, 0.012), (0.028, 4.5, 0.018), MAT["trim"], ARCH, 0)
add_box("OH_BACK_WALL", (0, 4.45, 1.75), (10, 0.20, 3.5), MAT["wall"])
add_box("OH_LEFT_WALL", (-4.95, 0, 1.75), (0.20, 9, 3.5), MAT["wall"])
add_box("OH_RIGHT_WALL", (4.95, 0, 1.75), (0.20, 9, 3.5), MAT["wall"])
add_box("OH_STOREFRONT_LEFT", (-3.05, -4.43, 1.75), (3.85, 0.18, 3.5), MAT["trim"], CUTAWAY)
add_box("OH_STOREFRONT_RIGHT", (3.05, -4.43, 1.75), (3.85, 0.18, 3.5), MAT["trim"], CUTAWAY)
add_box("OH_STOREFRONT_HEADER", (0, -4.43, 3.15), (2.25, 0.18, 0.70), MAT["trim"], CUTAWAY)
add_box("OH_WINDOW_LEFT", (-3.05, -4.33, 1.55), (3.25, 0.055, 2.35), MAT["glass"], CUTAWAY, 0.01)
add_box("OH_WINDOW_RIGHT", (3.05, -4.33, 1.55), (3.25, 0.055, 2.35), MAT["glass"], CUTAWAY, 0.01)
add_box("OH_SIGN_BOARD", (0, -4.57, 3.85), (6.4, 0.28, 0.75), MAT["wall"], CUTAWAY)
add_text("OH_SIGN_LETTERS", "ODD HOURS", (0, -4.75, 3.84), 0.62, 0.045, MAT["neon"], CUTAWAY)
add_text("OH_WINDOW_DECAL", "OPEN  24/7", (2.85, -4.40, 1.62), 0.28, 0.015, MAT["paper"], CUTAWAY)

# Hinged door whose object origin is the actual runtime hinge.
width, height, thickness = 1.45, 2.35, 0.10
verts = [(x, y, z) for x in (0, width) for y in (-thickness / 2, thickness / 2) for z in (0, height)]
faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
mesh = bpy.data.meshes.new("OH_DOOR_MESH")
mesh.from_pydata(verts, [], faces)
door = bpy.data.objects.new("DOOR_ODD_HOURS_HERO", mesh)
door.location = (-width / 2, -4.24, 0.0)
door.data.materials.append(MAT["door"])
CUTAWAY.objects.link(door)
door_glass = add_box("OH_DOOR_GLASS", (0, -4.30, 1.45), (0.82, 0.035, 1.22), MAT["glass"], CUTAWAY, 0.01)
door_glass.parent = door
door_glass.location = (width * 0.50, -0.06, 1.45)
handle = add_cylinder("OH_DOOR_HANDLE", (0.48, -4.38, 1.08), 0.045, 0.34, MAT["gold"], CUTAWAY, 12)
handle.rotation_euler = (math.pi / 2, 0, 0)
handle.parent = door
handle.location = (width * 0.83, -0.14, 1.08)

# Checkout counter, register plinth, package, impulse displays.
add_box("OH_CHECKOUT_COUNTER", (2.85, 2.30, 0.50), (2.75, 1.25, 1.0), MAT["counter"], FIXTURES, 0.10)
add_box("OH_CHECKOUT_TOP", (2.85, 2.30, 1.06), (2.90, 1.38, 0.13), MAT["countertop"], FIXTURES, 0.06)
add_box("OH_REGISTER_PLINTH", (3.35, 2.35, 1.18), (0.75, 0.55, 0.16), MAT["trim"], FIXTURES, 0.04)
package = add_box("PROP_COUNTER_PACKAGE", (1.70, 1.98, 1.22), (0.34, 0.24, 0.28), MAT["package"], PROPS, 0.035)
package_label = add_text("OH_PACKAGE_LABEL", "MARA", (2.02, 1.84, 1.22), 0.09, 0.008, MAT["paper"], PROPS)
package_label.parent = package
package_label.location = (0.0, -0.14, 0.0)
for index, x in enumerate((1.75, 2.05, 2.35)):
    add_box(f"OH_IMPULSE_PRODUCT_{index}", (x, 2.78, 1.22), (0.18, 0.22, 0.30 + index * 0.04), PRODUCT_MATS[index], PRODUCTS, 0.025)

# Shelves with real silhouette density and fictional price cards.
def shelving(name, center, dimensions, rows=4, columns=5):
    x, y = center
    w, d, h = dimensions
    add_box(f"{name}_FRAME", (x, y, h / 2), (w, d, h), MAT["shelf"], FIXTURES, 0.055)
    for row in range(rows):
        z = 0.28 + row * (h - 0.40) / max(1, rows - 1)
        add_box(f"{name}_SHELF_{row}", (x + w * 0.32, y, z), (w * 0.58, d * 1.08, 0.055), MAT["cream"], FIXTURES, 0.018)
        for column in range(columns):
            px = x + w * 0.10 + (column - (columns - 1) / 2) * (w * 0.10)
            py = y - d * 0.57
            product_height = 0.18 + 0.05 * ((row + column) % 3)
            add_box(f"{name}_PRODUCT_{row}_{column}", (px, py, z + product_height / 2 + 0.04), (w * 0.075, 0.14, product_height), PRODUCT_MATS[(row + column) % len(PRODUCT_MATS)], PRODUCTS, 0.018)
        add_box(f"{name}_PRICE_{row}", (x + w * 0.31, y - d * 0.62, z + 0.02), (0.30, 0.025, 0.12), MAT["paper"], PRODUCTS, 0.008)


shelving("OH_SHELF_WEST", (-2.75, -0.15), (1.05, 3.7, 1.65), 4, 6)
shelving("OH_SHELF_CENTER", (-0.55, 0.65), (1.0, 2.55, 1.55), 4, 5)
shelving("OH_ENDCAP", (2.95, -1.55), (0.90, 1.05, 1.35), 3, 4)

# Refrigerated wall and vending fixtures.
for index, x in enumerate((-3.45, -2.10, -0.75)):
    add_box(f"OH_FRIDGE_{index}", (x, 3.93, 1.18), (1.15, 0.78, 2.32), MAT["cream"], FIXTURES, 0.075)
    add_box(f"OH_FRIDGE_GLASS_{index}", (x, 3.48, 1.22), (0.88, 0.055, 1.75), MAT["glass"], FIXTURES, 0.018)
    for row in range(3):
        for col in range(3):
            add_box(f"OH_COLD_PRODUCT_{index}_{row}_{col}", (x + (col - 1) * 0.25, 3.40, 0.55 + row * 0.48), (0.13, 0.10, 0.28), PRODUCT_MATS[(index + row + col) % 5], PRODUCTS, 0.016)
add_box("OH_VENDING_MACHINE", (4.20, 0.05, 1.02), (1.15, 0.80, 2.04), MAT["trim"], FIXTURES, 0.085)
add_box("OH_VENDING_GLOW", (4.20, -0.38, 1.25), (0.82, 0.045, 1.18), MAT["neon"], FIXTURES, 0.015)
add_text("OH_VENDING_WORDMARK", "MOON POP", (4.20, -0.43, 0.42), 0.16, 0.015, MAT["paper"], FIXTURES)

# Storytelling cluster: a tiny midnight-owl notice board and lost-keys hook.
add_box("OH_STORY_BOARD", (-4.72, 2.40, 1.50), (0.10, 1.45, 1.40), MAT["cream"], FIXTURES, 0.035)
add_text("OH_STORY_HEADLINE", "NIGHT OWL\nNOTICE BOARD", (-4.64, 2.40, 1.82), 0.15, 0.012, MAT["wall"], FIXTURES, rotation=(math.pi / 2, 0, -math.pi / 2))
for index, z in enumerate((1.28, 1.48, 1.68)):
    add_box(f"OH_STORY_CARD_{index}", (-4.62, 2.12 + index * 0.22, z), (0.025, 0.26, 0.16), PRODUCT_MATS[index + 1], PRODUCTS, 0.006)
add_cylinder("OH_LOST_KEYS_HOOK", (-4.58, 2.87, 1.15), 0.035, 0.24, MAT["gold"], PROPS, 10).rotation_euler = (0, math.pi / 2, 0)

# Exterior dressing and recognizable street identity.
for index, x in enumerate((-3.6, 3.6)):
    add_cylinder(f"OH_EXTERIOR_BOLLARD_{index}", (x, -5.55, 0.42), 0.13, 0.84, MAT["gold"], FIXTURES, 12)
add_box("OH_NEWSPAPER_BOX", (-4.0, -6.3, 0.52), (0.62, 0.46, 1.04), MAT["counter"], FIXTURES, 0.08)
add_text("OH_NEWSPAPER_LABEL", "LATE EDITION", (-4.0, -6.56, 0.70), 0.13, 0.012, MAT["paper"], FIXTURES)
add_box("OH_TRASH_BIN", (4.15, -6.05, 0.52), (0.62, 0.62, 1.04), MAT["trim"], FIXTURES, 0.09)
add_box("OH_WINDOW_DISPLAY_PLINTH", (-2.8, -3.82, 0.42), (2.15, 0.68, 0.84), MAT["counter"], FIXTURES, 0.06)
for index in range(5):
    add_box(f"OH_WINDOW_DISPLAY_PRODUCT_{index}", (-3.45 + index * 0.32, -3.86, 1.02 + 0.08 * (index % 2)), (0.18, 0.18, 0.46), PRODUCT_MATS[index], PRODUCTS, 0.025)

# Actual adapted Poly Haven 3D cash register.
def import_and_normalize(path: Path, name: str, location, target_size: float):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    imported = [obj for obj in bpy.data.objects if obj not in before and obj.type == "MESH"]
    if not imported:
        raise RuntimeError(f"no mesh imported from {path}")
    parent = bpy.data.objects.new(name, None)
    PROPS.objects.link(parent)
    for obj in imported:
        obj.parent = parent
        move_to_collection(obj, PROPS)
    points = []
    for obj in imported:
        points.extend(obj.matrix_world @ Vector(corner) for corner in obj.bound_box)
    minimum = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
    maximum = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
    size = maximum - minimum
    factor = target_size / max(size)
    center = (minimum + maximum) * 0.5
    parent.scale = (factor, factor, factor)
    parent.location = Vector(location) - center * factor
    parent["polyhaven_asset_id"] = path.parent.name
    parent["license"] = "CC0"
    parent["adaptation"] = "scale-normalized, palette-integrated, runtime GLB"
    return parent


import_and_normalize(ROOT / "assets/source/polyhaven/CashRegister_01/CashRegister_01_1k.gltf", "PH_CASH_REGISTER_ADAPTED_HERO", (3.35, 2.32, 1.42), 0.55)
import_and_normalize(ROOT / "assets/source/polyhaven/plastic_crate_01/plastic_crate_01_1k.gltf", "PH_PLASTIC_CRATE_ADAPTED_HERO", (-3.85, 3.15, 0.32), 0.65)

# Ceiling lights and Bevy-compatible light sources.
for index, (x, y) in enumerate(((-2.6, -1.8), (0.0, -1.8), (2.6, -1.8), (-2.6, 1.7), (0.0, 1.7), (2.6, 1.7))):
    add_box(f"OH_CEILING_PANEL_{index}", (x, y, 3.25), (1.35, 0.42, 0.055), MAT["light"], LIGHTS, 0.025)
    data = bpy.data.lights.new(f"OH_LIGHT_DATA_{index}", "AREA")
    data.energy = 380
    data.color = (1.0, 0.78, 0.55)
    data.shape = "RECTANGLE"
    data.size = 2.0
    light = bpy.data.objects.new(f"LIGHT_STORE_{index}", data)
    light.location = (x, y, 3.05)
    light.rotation_euler = (0, 0, 0)
    LIGHTS.objects.link(light)

# Semantic helpers remain editable in Blender but are excluded from the runtime GLB.
def semantic(name, location, kind, metadata=None):
    obj = bpy.data.objects.new(name, None)
    obj.location = location
    obj.empty_display_type = "CUBE" if "REGION" in kind or "COLLIDER" in kind else "ARROWS"
    obj.empty_display_size = 0.35
    obj["semantic_type"] = kind
    if metadata:
        for key, value in metadata.items():
            obj[key] = value
    SEMANTICS.objects.link(obj)
    obj.hide_render = True
    return obj


semantic("NAV_REGION_ODD_HOURS_EXTERIOR", (0, -6.7, 0), "NAV_REGION", {"surface_type": "sidewalk"})
semantic("NAV_REGION_ODD_HOURS_INTERIOR", (0, 0, 0), "NAV_REGION", {"surface_type": "interior"})
semantic("NAV_PORTAL_ODD_HOURS_FRONT_DOOR", (0, -4.32, 0), "NAV_PORTAL", {"runtime_open": False, "control_entity": "DOOR_ODD_HOURS_HERO"})
semantic("ODD_HOURS_EXTERIOR_APPROACH", (2.8, -7.45, 0), "STAGING_MARK")
semantic("ODD_HOURS_DOOR_HANDLE_SIDE", (0.15, -4.80, 0), "STAGING_MARK")
semantic("ODD_HOURS_ENTRY", (0.15, -3.55, 0), "STAGING_MARK")
semantic("ODD_HOURS_COUNTER_APPROACH", (0.90, 1.20, 0), "STAGING_MARK")
semantic("ODD_HOURS_COUNTER_INTERACTION", (0.90, 1.90, 0), "STAGING_MARK")
semantic("INTERACT_ODD_HOURS_DOOR_HANDLE", (0.48, -4.38, 1.08), "INTERACTION_VOLUME")
semantic("INTERACT_ODD_HOURS_PACKAGE", (1.70, 1.98, 1.22), "INTERACTION_VOLUME")
for name, location in [
    ("CAM_OH_EXTERIOR_WIDE", (7.0, -10.5, 2.8)),
    ("CAM_OH_DOOR_MEDIUM", (4.1, -7.0, 2.1)),
    ("CAM_OH_INTERIOR_WIDE", (4.45, -2.2, 2.65)),
    ("CAM_OH_COUNTER_MEDIUM", (0.0, 3.8, 2.0)),
]:
    semantic(name, location, "CAMERA_ANCHOR")

# Preview camera.
bpy.ops.object.camera_add(location=(9.6, -12.4, 7.6))
camera = bpy.context.object
camera.name = "CAM_ODD_HOURS_HERO_PREVIEW"
move_to_collection(camera, LIGHTS)
def look_at(obj, target):
    obj.rotation_euler = (Vector(target) - obj.location).to_track_quat("-Z", "Y").to_euler()
look_at(camera, (0, 0, 1.15))
bpy.context.scene.camera = camera
bpy.context.scene.render.engine = "BLENDER_EEVEE"
bpy.context.scene.render.resolution_x = 960
bpy.context.scene.render.resolution_y = 540
bpy.context.scene.render.resolution_percentage = 100
bpy.context.scene.render.image_settings.file_format = "PNG"
bpy.context.scene.render.filepath = str(PREVIEW)
bpy.context.scene.world.color = (0.015, 0.02, 0.04)

# Save editable source, render exactly one art preview, then export visible production geometry.
PREVIEW.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE))
bpy.ops.render.render(write_still=True)
SEMANTICS.hide_viewport = True
bpy.context.view_layer.update()
GLB.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(GLB),
    export_format="GLB",
    use_visible=True,
    export_apply=True,
    export_lights=False,
    export_cameras=False,
)
SEMANTICS.hide_viewport = False
sha = hashlib.sha256(GLB.read_bytes()).hexdigest()

colliders = [
    {"id":"COLLIDER_STOREFRONT_LEFT","shape":"box","center":[-3.05,1.75,4.43],"half_extents":[1.93,1.75,0.10],"role":"static"},
    {"id":"COLLIDER_STOREFRONT_RIGHT","shape":"box","center":[3.05,1.75,4.43],"half_extents":[1.93,1.75,0.10],"role":"static"},
    {"id":"COLLIDER_DOOR_FRAME_LEFT","shape":"box","center":[-0.86,1.25,4.32],"half_extents":[0.10,1.25,0.12],"role":"static"},
    {"id":"COLLIDER_DOOR_FRAME_RIGHT","shape":"box","center":[0.86,1.25,4.32],"half_extents":[0.10,1.25,0.12],"role":"static"},
    {"id":"COLLIDER_DOOR_HEADER","shape":"box","center":[0.0,2.75,4.32],"half_extents":[0.96,0.30,0.12],"role":"static"},
    {"id":"COLLIDER_DOOR_CLOSED","shape":"box","center":[0.0,1.18,4.24],"half_extents":[0.73,1.18,0.07],"role":"dynamic"},
    {"id":"COLLIDER_SHELF_WEST","shape":"box","center":[-2.75,0.83,0.15],"half_extents":[0.53,0.83,1.86],"role":"static"},
    {"id":"COLLIDER_SHELF_CENTER","shape":"box","center":[-0.55,0.78,-0.65],"half_extents":[0.50,0.78,1.28],"role":"static"},
    {"id":"COLLIDER_ENDCAP","shape":"box","center":[2.95,0.68,1.55],"half_extents":[0.45,0.68,0.53],"role":"static"},
    {"id":"COLLIDER_COUNTER","shape":"box","center":[2.85,0.55,-2.30],"half_extents":[1.46,0.55,0.69],"role":"static"},
    {"id":"COLLIDER_FRIDGE_BANK","shape":"box","center":[-2.10,1.18,-3.93],"half_extents":[2.00,1.18,0.42],"role":"static"},
    {"id":"COLLIDER_VENDING","shape":"box","center":[4.20,1.02,-0.05],"half_extents":[0.58,1.02,0.40],"role":"static"},
    {"id":"COLLIDER_WINDOW_DISPLAY","shape":"box","center":[-2.80,0.42,3.82],"half_extents":[1.08,0.42,0.34],"role":"static"},
    {"id":"COLLIDER_EXTERIOR_BOLLARD_LEFT","shape":"box","center":[-3.60,0.42,5.55],"half_extents":[0.15,0.42,0.15],"role":"static"},
    {"id":"COLLIDER_EXTERIOR_BOLLARD_RIGHT","shape":"box","center":[3.60,0.42,5.55],"half_extents":[0.15,0.42,0.15],"role":"static"},
    {"id":"COLLIDER_NEWSPAPER_BOX","shape":"box","center":[-4.00,0.52,6.30],"half_extents":[0.31,0.52,0.23],"role":"static"},
    {"id":"COLLIDER_TRASH_BIN","shape":"box","center":[4.15,0.52,6.05],"half_extents":[0.31,0.52,0.31],"role":"static"},
]
for value in colliders:
    semantic(value["id"], (value["center"][0], -value["center"][2], value["center"][1]), "COLLIDER", value)
bpy.ops.wm.save_as_mainfile(filepath=str(SOURCE))

staging = [
    ("ODD_HOURS_EXTERIOR_APPROACH", [2.8,0.0,7.45]),
    ("ODD_HOURS_DOOR_HANDLE_SIDE", [0.15,0.0,4.80]),
    ("ODD_HOURS_ENTRY", [0.15,0.0,3.55]),
    ("ODD_HOURS_COUNTER_APPROACH", [0.90,0.0,-1.20]),
    ("ODD_HOURS_COUNTER_INTERACTION", [0.90,0.0,-1.90]),
]
cameras = [
    ("CAM_OH_EXTERIOR_WIDE", [7.0,2.8,10.5], [0.8,1.0,5.6]),
    ("CAM_OH_DOOR_MEDIUM", [4.1,2.1,7.0], [0.0,1.0,4.25]),
    ("CAM_OH_INTERIOR_WIDE", [4.45,2.65,2.2], [0.7,1.0,-0.2]),
    ("CAM_OH_COUNTER_MEDIUM", [0.0,2.0,-3.8], [1.7,1.05,-2.0]),
]
sidecar = {
    "schema_version":1,"module_id":"location_odd_hours_v3","asset":"assets/world/locations/location_odd_hours_v3.glb",
    "source_blend":"assets/source/blender/world/locations/location_odd_hours_v3.blend","category":"recurring_location","version":2,
    "quality_tier":"hero_production","bounds":{"min":[-5.0,-0.25,-4.5],"max":[5.0,4.2,9.0]},
    "sockets":[{"id":"NAV_PORTAL_ODD_HOURS_FRONT_DOOR","node":"DOOR_ODD_HOURS_HERO","position":[0.0,0.0,4.32],"width":1.5,"runtime_open":False,"control_entity":"DOOR_ODD_HOURS_HERO"}],
    "staging_marks":[{"id":id_,"node":id_,"position":position} for id_,position in staging],
    "camera_anchors":[{"id":id_,"node":id_,"position":position,"look_at":look} for id_,position,look in cameras],
    "interactions":[
        {"id":"INTERACT_ODD_HOURS_DOOR_HANDLE","node":"OH_DOOR_HANDLE","position":[0.48,1.08,4.38],"interaction_type":"door_handle","smart_interaction":"SMART_DOOR_OPEN"},
        {"id":"INTERACT_ODD_HOURS_PACKAGE","node":"PROP_COUNTER_PACKAGE","position":[1.70,1.22,-1.98],"interaction_type":"pickup","smart_interaction":"SMART_PICKUP_SMALL"},
    ],
    "cutaway_groups":[{"id":"CUTAWAY_FRONT","node":"CUTAWAY_FRONT","position":[0.0,1.8,4.43]}],
    "collision_groups":[{"id":c["id"],"node":c["id"],"position":c["center"],"shape":c["shape"],"half_extents":c["half_extents"],"role":c["role"]} for c in colliders],
    "lighting":[{"id":f"LIGHT_STORE_{i}","role":"LIGHT_STORE","light_type":"point","position":[x,3.05,-y],"direction":[0,-1,0],"color_rgb":[1.0,0.78,0.55],"intensity":500.0,"range":8.0,"runtime_controlled":False} for i,(x,y) in enumerate(((-2.6,-1.8),(0.0,-1.8),(2.6,-1.8),(-2.6,1.7),(0.0,1.7),(2.6,1.7)))],
    "runtime_controls":[
        {"id":"DOOR_ODD_HOURS_HERO","node":"DOOR_ODD_HOURS_HERO","kind":"hinged_door","default_state":"closed"},
        {"id":"PROP_COUNTER_PACKAGE","node":"PROP_COUNTER_PACKAGE","kind":"pickup_prop","default_state":"counter"},
    ],
    "walkable_regions":[
        {"id":"NAV_REGION_ODD_HOURS_EXTERIOR","polygon":[[-4.7,8.8],[4.7,8.8],[4.7,4.5],[-4.7,4.5]],"height":0.0,"surface_type":"sidewalk","actor_clearance":0.34},
        {"id":"NAV_REGION_ODD_HOURS_INTERIOR","polygon":[[-4.7,4.2],[4.7,4.2],[4.7,-4.2],[-4.7,-4.2]],"height":0.0,"surface_type":"interior","actor_clearance":0.34},
    ],
    "portals":[{"id":"NAV_PORTAL_ODD_HOURS_FRONT_DOOR","position":[0.0,0.0,4.32],"width":1.5,"regions":["NAV_REGION_ODD_HOURS_EXTERIOR","NAV_REGION_ODD_HOURS_INTERIOR"],"runtime_open":False,"control_entity":"DOOR_ODD_HOURS_HERO"}],
    "colliders":colliders,
    "runtime_doors":["DOOR_ODD_HOURS_HERO"],
    "provenance":{"author":"Infinite Backlot project","license":"project-owned adaptation with Poly Haven CC0 sources","source":"Odd Hours production hero upgrade","polyhaven_cc0_assets":["CashRegister_01","plastic_crate_01"],"manifest":"assets/world/kits/polyhaven_cc0_intake.provenance.json"},
    "preview":"assets/reference/production-vertical-slice/odd_hours_hero_preview.png","glb_sha256":sha,
}
SIDECAR.write_text(json.dumps(sidecar, indent=2)+"\n", encoding="utf-8")

regions = [
    {"id":"NAV_REGION_ODD_HOURS_EXTERIOR","surface_type":"sidewalk","access":"public","height":0.0,"max_slope_deg":3.0,"actor_clearance":0.34,"priority":1,"polygon":[[-4.7,8.8],[4.7,8.8],[4.7,4.5],[-4.7,4.5]],"connected_portals":["NAV_PORTAL_ODD_HOURS_FRONT_DOOR"]},
    {"id":"NAV_REGION_ODD_HOURS_INTERIOR","surface_type":"interior","access":"public","height":0.0,"max_slope_deg":3.0,"actor_clearance":0.34,"priority":2,"polygon":[[-4.7,4.2],[4.7,4.2],[4.7,-4.2],[-4.7,-4.2]],"connected_portals":["NAV_PORTAL_ODD_HOURS_FRONT_DOOR"]},
]
guides = [
    ("GUIDE_EXTERIOR_START","NAV_REGION_ODD_HOURS_EXTERIOR",[2.8,0.0,7.45],None),
    ("GUIDE_EXTERIOR_AROUND_BIN","NAV_REGION_ODD_HOURS_EXTERIOR",[2.1,0.0,6.15],None),
    ("GUIDE_DOOR_APPROACH","NAV_REGION_ODD_HOURS_EXTERIOR",[0.15,0.0,4.80],None),
    ("GUIDE_PORTAL_OUT","NAV_REGION_ODD_HOURS_EXTERIOR",[0.10,0.0,4.58],"NAV_PORTAL_ODD_HOURS_FRONT_DOOR"),
    ("GUIDE_PORTAL_IN","NAV_REGION_ODD_HOURS_INTERIOR",[0.10,0.0,3.82],"NAV_PORTAL_ODD_HOURS_FRONT_DOOR"),
    ("GUIDE_INTERIOR_ENTRY","NAV_REGION_ODD_HOURS_INTERIOR",[0.70,0.0,3.15],None),
    ("GUIDE_AISLE_TURN","NAV_REGION_ODD_HOURS_INTERIOR",[1.30,0.0,2.20],None),
    ("GUIDE_AISLE_MID","NAV_REGION_ODD_HOURS_INTERIOR",[1.35,0.0,0.55],None),
    ("GUIDE_COUNTER_APPROACH","NAV_REGION_ODD_HOURS_INTERIOR",[0.90,0.0,-1.20],None),
    ("GUIDE_COUNTER_INTERACTION","NAV_REGION_ODD_HOURS_INTERIOR",[0.90,0.0,-1.90],None),
]
nav = {
    "schema_version":1,"world_id":"odd_hours_production","coordinate_system":"bevy_y_up_meters",
    "actor_defaults":{"capsule_radius":0.34,"capsule_half_height":0.90,"floor_sample_step":0.10,"path_sample_step":0.10,"turn_radius":0.48},
    "regions":regions,
    "portals":[{"id":"NAV_PORTAL_ODD_HOURS_FRONT_DOOR","regions":["NAV_REGION_ODD_HOURS_EXTERIOR","NAV_REGION_ODD_HOURS_INTERIOR"],"position":[0.0,0.0,4.32],"facing":[0.0,0.0,-1.0],"width":1.5,"clearance":0.08,"traversal_type":"hinged_door","runtime_open":False,"control_entity":"DOOR_ODD_HOURS_HERO"}],
    "colliders":[c for c in colliders if c["role"] == "static"],
    "floor_supports":[
        {"id":"FLOOR_SUPPORT_EXTERIOR","region_id":"NAV_REGION_ODD_HOURS_EXTERIOR","height":0.0,"polygon":[[-4.7,8.8],[4.7,8.8],[4.7,4.5],[-4.7,4.5]]},
        {"id":"FLOOR_SUPPORT_DOOR_THRESHOLD","region_id":"NAV_REGION_ODD_HOURS_INTERIOR","height":0.0,"polygon":[[-0.9,4.65],[0.9,4.65],[0.9,3.75],[-0.9,3.75]]},
        {"id":"FLOOR_SUPPORT_INTERIOR","region_id":"NAV_REGION_ODD_HOURS_INTERIOR","height":0.0,"polygon":[[-4.7,4.2],[4.7,4.2],[4.7,-4.2],[-4.7,-4.2]]},
    ],
    "guide_nodes":[{"id":id_,"region_id":region,"position":position,"portal_id":portal} for id_,region,position,portal in guides],
    "guide_edges":[[guides[i][0],guides[i+1][0]] for i in range(len(guides)-1)],
    "interaction_volumes":[
        {"id":"INTERACTION_VOLUME_DOOR","interaction_id":"SMART_DOOR_OPEN","center":[0.15,0.9,5.0],"half_extents":[0.75,0.9,0.65],"required_clearance":0.62},
        {"id":"INTERACTION_VOLUME_COUNTER","interaction_id":"SMART_PICKUP_SMALL","center":[0.90,0.9,-1.90],"half_extents":[0.62,0.9,0.62],"required_clearance":0.52},
    ],
    "semantic_destinations":{id_:position for id_,position in staging},
}
NAV.write_text(json.dumps(nav, indent=2)+"\n", encoding="utf-8")

registry = json.loads(REGISTRY.read_text(encoding="utf-8"))
for module in registry["modules"]:
    if module["module_id"] == "location_odd_hours_v3":
        module.update({
            "asset":"assets/world/locations/location_odd_hours_v3.glb","source_blend":"assets/source/blender/world/locations/location_odd_hours_v3.blend",
            "version":2,"quality_tier":"hero_production","bounds":sidecar["bounds"],"sockets":sidecar["sockets"],"staging_marks":sidecar["staging_marks"],
            "camera_anchors":sidecar["camera_anchors"],"interactions":sidecar["interactions"],"cutaway_groups":sidecar["cutaway_groups"],
            "collision_groups":sidecar["collision_groups"],"lighting":sidecar["lighting"],"runtime_controls":sidecar["runtime_controls"],
            "glb_sha256":sha,"preview":sidecar["preview"],"provenance":sidecar["provenance"],
            "tags":["hero","convenience-store","odd-hours","navigation","smart-interaction","production-vertical-slice"],
        })
        break
else:
    raise RuntimeError("location_odd_hours_v3 missing from registry")
REGISTRY.write_text(json.dumps(registry, indent=2)+"\n", encoding="utf-8")
print(json.dumps({"source":str(SOURCE),"glb":str(GLB),"glb_sha256":sha,"preview":str(PREVIEW),"objects":len(bpy.data.objects),"colliders":len(colliders),"navigation":str(NAV)}))
