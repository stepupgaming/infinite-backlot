"""Build reusable Infinite Backlot indoor/outdoor modules in Blender.

Run through Blender MCP (preferred authoring path):
    exec(compile(open(r'C:/Projects/bevy-infinite/tools/blender/build_world_kits.py', encoding='utf-8').read(), 'build_world_kits.py', 'exec'))

Or deterministically in background:
    blender --background --python tools/blender/build_world_kits.py

Each module receives its own editable .blend, runtime .glb, semantic sidecar, and
low-cost preview. Geometry and materials are project-authored and dependency-free.
"""
from __future__ import annotations

import hashlib
import json
import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import bpy
from mathutils import Vector

ROOT = Path(r"C:/Projects/bevy-infinite")
SOURCE_ROOT = ROOT / "assets/source/blender/world"
RUNTIME_ROOT = ROOT / "assets/world"
PREVIEW_ROOT = ROOT / "assets/reference/world-modules"
REGISTRY_PATH = RUNTIME_ROOT / "registry.json"


@dataclass(frozen=True)
class ModuleSpec:
    module_id: str
    category: str
    kit: str
    style: str
    width: float
    depth: float
    height: float = 3.2
    sockets: tuple[str, ...] = ()
    tags: tuple[str, ...] = ()
    interactions: tuple[str, ...] = ()
    builder: str = "room"


INDOOR = (
    ModuleSpec("apartment_exterior_a", "building_exterior", "apartment_building", "brick", 14, 5, 11, ("SOCKET_EXTERIOR", "SOCKET_ROOFTOP"), ("hero", "exterior"), ("INTERACT_MAIN_DOOR",), "exterior"),
    ModuleSpec("apartment_main_entrance_a", "building_entrance", "apartment_building", "lobby", 7, 5, 4, ("SOCKET_EXTERIOR", "SOCKET_HALL_NORTH"), ("entrance", "camera_safe"), ("INTERACT_MAIN_DOOR",), "entrance"),
    ModuleSpec("apartment_lobby_a", "lobby", "apartment_building", "lobby", 9, 8, 3.6, ("SOCKET_HALL_NORTH", "SOCKET_HALL_SOUTH", "SOCKET_ELEVATOR"), ("conversation", "hero"), ("INTERACT_DIRECTORY",), "lobby"),
    ModuleSpec("apartment_elevator_lobby_a", "elevator_lobby", "apartment_building", "teal", 8, 6, 3.4, ("SOCKET_HALL_SOUTH", "SOCKET_ELEVATOR"), ("elevator", "reveal"), ("INTERACT_ELEVATOR_PANEL", "INTERACT_ELEVATOR_DOORS"), "elevator"),
    ModuleSpec("apartment_hall_straight_a", "indoor_hallway", "apartment_building", "burgundy", 4.8, 9, 3.2, ("SOCKET_HALL_NORTH", "SOCKET_HALL_SOUTH"), ("hall", "conversation"), (), "hall_straight"),
    ModuleSpec("apartment_hall_short_a", "indoor_hallway", "apartment_building", "teal", 4.8, 5, 3.2, ("SOCKET_HALL_NORTH", "SOCKET_HALL_SOUTH"), ("hall", "short"), (), "hall_straight"),
    ModuleSpec("apartment_hall_corner_a", "indoor_junction", "apartment_building", "burgundy", 6, 6, 3.2, ("SOCKET_HALL_SOUTH", "SOCKET_HALL_EAST"), ("corner", "blocking"), (), "hall_corner"),
    ModuleSpec("apartment_hall_t_junction_a", "indoor_junction", "apartment_building", "teal", 8, 6, 3.2, ("SOCKET_HALL_SOUTH", "SOCKET_HALL_EAST", "SOCKET_HALL_WEST"), ("junction", "blocking"), (), "hall_t"),
    ModuleSpec("apartment_doorway_section_a", "apartment_doorway", "apartment_building", "walnut", 5, 4, 3.2, ("SOCKET_HALL_NORTH", "SOCKET_HALL_SOUTH", "SOCKET_DOOR"), ("doorway", "entry"), ("INTERACT_APARTMENT_DOOR",), "doorway"),
    ModuleSpec("apartment_stairwell_a", "stairwell", "apartment_building", "service", 6, 7, 6.4, ("SOCKET_HALL_SOUTH", "SOCKET_STAIRS_UP", "SOCKET_STAIRS_DOWN"), ("stairs", "vertical"), ("INTERACT_STAIR_DOOR",), "stairwell"),
    ModuleSpec("apartment_laundry_a", "utility_room", "apartment_building", "laundry", 7, 6, 3.2, ("SOCKET_DOOR",), ("laundry", "conversation"), ("INTERACT_WASHER", "INTERACT_DRYER"), "laundry"),
    ModuleSpec("apartment_maintenance_a", "utility_room", "apartment_building", "service", 7, 6, 3.2, ("SOCKET_DOOR",), ("maintenance", "props"), ("INTERACT_WORKBENCH", "INTERACT_BREAKER_PANEL"), "maintenance"),
    ModuleSpec("apartment_boiler_room_a", "mechanical_room", "apartment_building", "industrial", 9, 7, 3.8, ("SOCKET_DOOR", "SOCKET_HALL_SOUTH"), ("basement", "mechanical"), ("INTERACT_BOILER", "INTERACT_VALVE"), "boiler"),
    ModuleSpec("apartment_rooftop_a", "rooftop", "apartment_building", "roof", 14, 11, 2.8, ("SOCKET_ROOFTOP", "SOCKET_STAIRS_DOWN"), ("exterior", "skyline", "conversation"), ("INTERACT_ROOFTOP_DOOR",), "rooftop"),
    ModuleSpec("apartment_interior_recurring_a", "apartment_interior", "apartment_building", "warm", 9, 8, 3.2, ("SOCKET_DOOR",), ("hero", "interior", "recurring"), ("INTERACT_SOFA", "INTERACT_KITCHEN_COUNTER"), "apartment"),
    ModuleSpec("apartment_service_loading_a", "service_entrance", "apartment_building", "industrial", 8, 6, 3.8, ("SOCKET_HALL_NORTH", "SOCKET_EXTERIOR", "LOT_LOADING"), ("service", "loading"), ("INTERACT_LOADING_DOOR",), "loading"),
)

OUTDOOR = (
    ModuleSpec("neighborhood_street_straight_a", "street", "neighborhood", "asphalt", 12, 24, 0.4, ("ROAD_NORTH", "ROAD_SOUTH", "SIDEWALK_EAST", "SIDEWALK_WEST"), ("street", "vehicle"), (), "street"),
    ModuleSpec("neighborhood_street_corner_a", "street_corner", "neighborhood", "asphalt", 18, 18, 0.4, ("ROAD_SOUTH", "ROAD_EAST", "SIDEWALK_NORTH", "SIDEWALK_WEST"), ("street", "corner"), (), "street_corner"),
    ModuleSpec("neighborhood_intersection_a", "intersection", "neighborhood", "asphalt", 22, 22, 0.4, ("ROAD_NORTH", "ROAD_SOUTH", "ROAD_EAST", "ROAD_WEST"), ("intersection", "crosswalk"), (), "intersection"),
    ModuleSpec("neighborhood_alley_a", "alley", "neighborhood", "industrial", 7, 18, 4.5, ("ALLEY_NORTH", "ALLEY_SOUTH", "LOT_SERVICE"), ("alley", "service"), ("INTERACT_DUMPSTER",), "alley"),
    ModuleSpec("neighborhood_courtyard_a", "courtyard", "neighborhood", "plaza", 18, 16, 2.0, ("SIDEWALK_NORTH", "ALLEY_EAST", "BUILDING_ENTRANCE_WEST"), ("courtyard", "conversation"), (), "courtyard"),
    ModuleSpec("neighborhood_parking_loading_a", "parking", "neighborhood", "asphalt", 18, 14, 1.0, ("LOT_STREET", "LOT_LOADING", "ALLEY_WEST"), ("parking", "loading"), (), "parking"),
    ModuleSpec("neighborhood_bus_stop_a", "transit", "neighborhood", "transit", 12, 6, 3.0, ("SIDEWALK_EAST", "SIDEWALK_WEST", "TRANSIT_BUS"), ("bus_stop", "conversation"), ("INTERACT_BUS_SIGN",), "bus_stop"),
    ModuleSpec("neighborhood_pocket_park_a", "park", "neighborhood", "park", 16, 14, 3.0, ("SIDEWALK_NORTH", "SIDEWALK_SOUTH"), ("park", "planted"), ("INTERACT_BENCH",), "park"),
    ModuleSpec("neighborhood_storefront_row_a", "storefront_row", "neighborhood", "storefront", 22, 6, 6.5, ("SIDEWALK_SOUTH", "BUILDING_ENTRANCE_A", "BUILDING_ENTRANCE_B", "BUILDING_ENTRANCE_C"), ("facade", "business"), (), "storefront"),
    ModuleSpec("neighborhood_convenience_store_a", "hero_business", "neighborhood", "convenience", 10, 9, 4.0, ("SIDEWALK_SOUTH", "BUILDING_ENTRANCE_MAIN", "ALLEY_NORTH"), ("hero", "business", "interior_shell"), ("INTERACT_STORE_DOOR", "INTERACT_COUNTER"), "store_shell"),
    ModuleSpec("neighborhood_laundromat_a", "business_exterior", "neighborhood", "laundromat", 9, 6, 4.5, ("SIDEWALK_SOUTH", "BUILDING_ENTRANCE_MAIN"), ("business", "facade"), ("INTERACT_LAUNDROMAT_DOOR",), "business"),
    ModuleSpec("neighborhood_diner_a", "business_exterior", "neighborhood", "diner", 12, 7, 5.0, ("SIDEWALK_SOUTH", "BUILDING_ENTRANCE_MAIN", "ALLEY_EAST"), ("business", "facade", "hero_candidate"), ("INTERACT_DINER_DOOR",), "business"),
    ModuleSpec("neighborhood_skyline_facades_a", "skyline_proxy", "neighborhood", "skyline", 32, 7, 18, ("LOT_BACKGROUND",), ("distant", "proxy", "skyline"), (), "skyline"),
)

ALL_MODULES = INDOOR + OUTDOOR

PALETTE = {
    "ivory": (0.58, 0.50, 0.40, 1), "teal": (0.035, 0.19, 0.20, 1),
    "burgundy": (0.27, 0.035, 0.07, 1), "brass": (0.48, 0.27, 0.055, 1),
    "walnut": (0.22, 0.065, 0.025, 1), "concrete": (0.22, 0.24, 0.25, 1),
    "asphalt": (0.055, 0.065, 0.075, 1), "roadline": (0.92, 0.67, 0.12, 1),
    "sidewalk": (0.36, 0.38, 0.37, 1), "brick": (0.42, 0.095, 0.055, 1),
    "glass": (0.05, 0.20, 0.28, 1), "green": (0.06, 0.30, 0.12, 1),
    "red": (0.60, 0.045, 0.03, 1), "blue": (0.035, 0.16, 0.48, 1),
    "cream": (0.83, 0.68, 0.42, 1), "black": (0.012, 0.018, 0.025, 1),
    "white": (0.78, 0.80, 0.77, 1), "orange": (0.92, 0.24, 0.035, 1),
}


def reset_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (bpy.data.meshes, bpy.data.curves, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for block in list(datablocks):
            datablocks.remove(block)
    for collection in list(bpy.data.collections):
        if collection.name != "Collection":
            bpy.data.collections.remove(collection)
    root = bpy.context.scene.collection
    base = bpy.data.collections.get("Collection")
    if base:
        base.name = "MODULE"
    else:
        root.children.link(bpy.data.collections.new("MODULE"))
    for name in ("SEMANTICS", "CAMERAS", "COLLIDERS", "CUTAWAYS", "PREVIEW"):
        root.children.link(bpy.data.collections.new(name))


def mat(name: str, key: str, *, metallic: float = 0.0, roughness: float = 0.68, emission: float = 0.0):
    m = bpy.data.materials.new(f"MAT_{name}")
    color = PALETTE[key]
    m.diffuse_color = color
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = color
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    if emission:
        bsdf.inputs["Emission Color"].default_value = color
        bsdf.inputs["Emission Strength"].default_value = emission
    m["gltf_compatible"] = True
    return m


def link_collection(obj, name: str) -> None:
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    bpy.data.collections[name].objects.link(obj)


def cube(name, loc, dims, material, *, collection="MODULE", bevel=0.05, kind="static"):
    bpy.ops.mesh.primitive_cube_add(location=loc)
    o = bpy.context.object
    o.name = name
    o.dimensions = dims
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    o.data.materials.append(material)
    if bevel:
        mod = o.modifiers.new("Production bevel", "BEVEL")
        mod.width = min(bevel, min(dims) * 0.18)
        mod.segments = 2
    o["semantic_kind"] = kind
    link_collection(o, collection)
    return o


def cyl(name, loc, radius, depth, material, *, collection="MODULE", vertices=16, kind="static"):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=loc)
    o = bpy.context.object
    o.name = name
    o.data.materials.append(material)
    o["semantic_kind"] = kind
    link_collection(o, collection)
    return o


def semantic(name, loc, kind, **props):
    o = bpy.data.objects.new(name, None)
    bpy.data.collections["SEMANTICS"].objects.link(o)
    o.location = loc
    o.empty_display_type = "ARROWS" if kind == "socket" else "CIRCLE"
    o.empty_display_size = 0.34
    o["semantic_kind"] = kind
    o["semantic_id"] = name
    for key, value in props.items():
        o[key] = value
    return o


def camera(name, loc, target, lens=42.0):
    data = bpy.data.cameras.new(name)
    data.lens = lens
    data.sensor_width = 36
    o = bpy.data.objects.new(name, data)
    bpy.data.collections["CAMERAS"].objects.link(o)
    o.location = loc
    o.rotation_euler = (Vector(target) - o.location).to_track_quat("-Z", "Y").to_euler()
    o["semantic_kind"] = "camera_anchor"
    o["semantic_id"] = name
    o["look_at"] = list(target)
    return o


def collider(name, loc, dims):
    o = cube(name, loc, dims, mat("Collider", "black"), collection="COLLIDERS", bevel=0, kind="collider")
    o.hide_render = True
    o.display_type = "WIRE"
    o["collision_role"] = "solid"
    return o


def socket_location(socket_id: str, spec: ModuleSpec):
    w, d = spec.width, spec.depth
    if any(k in socket_id for k in ("NORTH", "ROOFTOP", "UP")):
        return (0, d / 2, 0)
    if any(k in socket_id for k in ("SOUTH", "EXTERIOR", "DOWN", "STREET")):
        return (0, -d / 2, 0)
    if any(k in socket_id for k in ("EAST", "_A", "MAIN")):
        return (w / 2, 0, 0)
    if any(k in socket_id for k in ("WEST", "_B")):
        return (-w / 2, 0, 0)
    return (0, 0, 0)


def add_contract(spec: ModuleSpec):
    for socket_id in spec.sockets:
        semantic(socket_id, socket_location(socket_id, spec), "socket", socket_type=socket_id.split("_")[0], clearance_m=1.5)
    marks = [
        ("MARK_ENTRY", (0, -spec.depth * 0.30, 0)),
        ("MARK_EXIT", (0, spec.depth * 0.30, 0)),
        ("MARK_CONVERSATION_A", (-1.0, 0, 0)),
        ("MARK_CONVERSATION_B", (1.0, 0, 0)),
        ("MARK_OBSERVER", (0, min(1.6, spec.depth * 0.22), 0)),
        ("MARK_REVEAL_CLEAR", (min(1.8, spec.width * 0.28), min(1.5, spec.depth * 0.22), 0)),
    ]
    for name, loc in marks:
        semantic(name, loc, "staging_mark", radius_m=0.55)
    for iid in spec.interactions:
        semantic(iid, (spec.width * 0.30, -spec.depth * 0.28, 1.0), "interaction", interaction_type=iid.removeprefix("INTERACT_").lower())
    camera("CAM_WIDE", (spec.width * 0.72, -spec.depth * 0.78, max(2.5, spec.height * 0.62)), (0, 0, 1.1), 38)
    camera("CAM_TWO_SHOT", (-spec.width * 0.45, -spec.depth * 0.62, 1.7), (0, 0, 1.15), 52)
    camera("CAM_INTERACTION", (spec.width * 0.34, -spec.depth * 0.44, 1.65), (spec.width * 0.28, -spec.depth * 0.20, 1.0), 62)
    camera("CAM_REVEAL", (-spec.width * 0.62, -spec.depth * 0.55, 2.1), (0, spec.depth * 0.28, 1.2), 45)
    collider("COLLIDER_FLOOR", (0, 0, -0.18), (spec.width, spec.depth, 0.3))


def materials():
    return {
        "wall": mat("Wall", "ivory"), "accent": mat("Accent", "teal"),
        "floor": mat("Floor", "concrete"), "concrete": mat("Concrete", "concrete"),
        "trim": mat("Trim", "brass", metallic=.65, roughness=.3),
        "wood": mat("Wood", "walnut"), "metal": mat("Metal", "concrete", metallic=.72, roughness=.28),
        "glass": mat("Glass", "glass", metallic=.2, roughness=.18), "brick": mat("Brick", "brick"),
        "road": mat("Road", "asphalt"), "roadline": mat("RoadLine", "roadline", roughness=.5),
        "sidewalk": mat("Sidewalk", "sidewalk"), "green": mat("Green", "green"),
        "red": mat("Red", "red"), "blue": mat("Blue", "blue"), "cream": mat("Cream", "cream"),
        "black": mat("Black", "black"), "white": mat("White", "white"),
        "orange": mat("OrangeGlow", "orange", emission=2.5),
    }


def room_shell(spec, m, *, front_cutaway=True):
    w, d, h = spec.width, spec.depth, spec.height
    cube("SET_Floor", (0, 0, -.08), (w, d, .16), m["floor"], bevel=.025)
    cube("SET_Wall_Back", (0, d/2, h/2), (w, .18, h), m["wall"])
    cube("SET_Wall_Left", (-w/2, 0, h/2), (.18, d, h), m["wall"])
    cube("SET_Wall_Right", (w/2, 0, h/2), (.18, d, h), m["wall"])
    if front_cutaway:
        front = cube("CUTAWAY_FRONT", (0, -d/2, h/2), (w, .15, h), m["wall"], collection="CUTAWAYS", kind="cutaway")
        front.hide_render = True
    cube("SET_Baseboard_Back", (0, d/2-.12, .11), (w, .08, .22), m["trim"], bevel=.02)
    cube("SET_Trim_Left", (-w/2+.12, 0, .11), (.08, d, .22), m["trim"], bevel=.02)
    cube("SET_Trim_Right", (w/2-.12, 0, .11), (.08, d, .22), m["trim"], bevel=.02)
    for x in (-w*.25, w*.25):
        cube(f"SET_LightFixture_{'L' if x<0 else 'R'}", (x, 0, h-.12), (.65, .24, .10), m["cream"], bevel=.04)


def door(name, x, y, m, *, width=1.25, height=2.35):
    cube(f"SET_{name}_Door", (x, y, height/2), (width, .12, height), m["wood"], bevel=.035)
    cube(f"SET_{name}_Header", (x, y-.02, height+.09), (width+.28, .20, .18), m["trim"], bevel=.025)
    for dx in (-width/2-.08, width/2+.08):
        cube(f"SET_{name}_Jamb_{'L' if dx<0 else 'R'}", (x+dx, y-.02, height/2), (.16, .20, height+.18), m["trim"], bevel=.025)
    cyl(f"PROP_{name}_Handle", (x+width*.32, y-.10, 1.05), .045, .15, m["trim"], vertices=12, kind="prop").rotation_euler[0] = math.radians(90)


def build_indoor(spec, m):
    if spec.builder == "exterior":
        cube("SET_BuildingMass", (0, 1.3, spec.height/2), (spec.width, spec.depth-2.6, spec.height), m["brick"], bevel=.15)
        for floor in range(4):
            z = 1.5 + floor*2.3
            for x in (-4.5, -1.5, 1.5, 4.5):
                cube(f"SET_Window_{floor}_{x:+.1f}", (x, -1.25, z), (1.35, .08, 1.25), m["glass"], bevel=.04)
        cube("SET_EntranceCanopy", (0, -2.1, 2.65), (4.0, 1.8, .22), m["trim"], bevel=.09)
        door("Main", 0, -2.05, m, width=1.8, height=2.55)
        cube("SET_RoofCornice", (0, 1.3, spec.height-.25), (spec.width+.4, spec.depth-2.3, .5), m["trim"], bevel=.08)
        cube("SET_SidewalkApron", (0, -2.5, -.05), (spec.width, 2.1, .10), m["sidewalk"], bevel=.03)
    elif spec.builder in {"hall_straight", "hall_corner", "hall_t", "doorway"}:
        room_shell(spec, m)
        cube("SET_CarpetRunner", (0, 0, .025), (2.1, spec.depth*.88, .05), m["accent"], bevel=.025)
        if spec.builder == "doorway":
            door("Apartment", spec.width*.27, spec.depth/2-.10, m)
        else:
            for i, y in enumerate((-spec.depth*.23, spec.depth*.23)):
                door(f"Apartment_{i+1}", spec.width/2-.12, y, m)
    elif spec.builder == "stairwell":
        room_shell(spec, m)
        for i in range(12):
            cube(f"SET_Stair_{i:02d}", (-1.1, -2.2+i*.36, .12+i*.20), (2.2, .36, .24), m["concrete" if "concrete" in m else "floor"], bevel=.025)
        cube("SET_Landing", (1.0, 2.2, 2.45), (3.7, 2.0, .20), m["floor"])
        cube("SET_Rail", (0.15, 0, 1.65), (.10, 5.0, .12), m["trim"], bevel=.04)
    elif spec.builder == "rooftop":
        cube("SET_RoofDeck", (0, 0, -.08), (spec.width, spec.depth, .16), m["concrete"], bevel=.04)
        for x, y, dx, dy in ((0,spec.depth/2,spec.width,.25),(0,-spec.depth/2,spec.width,.25),(spec.width/2,0,.25,spec.depth),(-spec.width/2,0,.25,spec.depth)):
            cube(f"SET_Parapet_{x}_{y}", (x,y,.65), (dx,dy,1.3), m["brick"], bevel=.06)
        cube("SET_RooftopEntry", (-3.6, 2.2, 1.45), (3.0, 3.0, 2.9), m["brick"], bevel=.09)
        door("Roof", -3.6, .65, m)
        for i, x in enumerate((-1.0, 2.0, 4.0)):
            cyl(f"SET_Vent_{i}", (x, 1.2-i*.6, .65), .42, 1.3, m["metal"], vertices=16)
    else:
        room_shell(spec, m)
        if spec.builder in {"entrance", "lobby", "elevator", "loading"}:
            door("Entry", 0, spec.depth/2-.10, m, width=1.65)
        if spec.builder == "elevator":
            cube("DOOR_Elevator_Left", (-.62, spec.depth/2-.13, 1.25), (1.18, .18, 2.5), m["metal"], kind="dynamic")
            cube("DOOR_Elevator_Right", (.62, spec.depth/2-.13, 1.25), (1.18, .18, 2.5), m["metal"], kind="dynamic")
            cube("PROP_Elevator_Panel", (1.55, spec.depth/2-.22, 1.25), (.35, .12, 1.0), m["black"], kind="interactable")
            cyl("PROP_Elevator_Button", (1.55, spec.depth/2-.30, 1.2), .08, .08, m["orange"], vertices=16, kind="interactable").rotation_euler[0]=math.radians(90)
            semantic("MARK_PANEL_OPERATOR", (1.1, spec.depth*.25, 0), "staging_mark", radius_m=.5)
        elif spec.builder == "laundry":
            for i, x in enumerate((-2.1, 0, 2.1)):
                cube(f"PROP_Washer_{i}", (x, 2.05, .65), (1.45, .85, 1.3), m["metal"], kind="prop")
                cyl(f"PROP_WasherDoor_{i}", (x, 1.58, .7), .42, .10, m["glass"], vertices=20, kind="prop").rotation_euler[0]=math.radians(90)
            cube("PROP_FoldingTable", (0, -.4, .85), (3.2, 1.0, .12), m["cream"], kind="prop")
        elif spec.builder == "maintenance":
            cube("PROP_Workbench", (0, 2.1, .9), (4.8, 1.0, .18), m["wood"], kind="prop")
            cube("PROP_BreakerPanel", (2.4, 2.82, 1.45), (1.0, .12, 1.5), m["metal"], kind="interactable")
            for i in range(4):
                cube(f"PROP_Toolbox_{i}", (-1.8+i*1.15, 1.85, 1.15), (.8,.42,.35), m["red"], kind="prop")
        elif spec.builder == "boiler":
            for i, x in enumerate((-2.4, 0, 2.4)):
                cyl(f"PROP_Boiler_{i}", (x, 1.3, 1.35), .75, 2.7, m["metal"], vertices=20, kind="interactable")
                cyl(f"PROP_Valve_{i}", (x, .48, 1.35), .25, .10, m["red"], vertices=12, kind="interactable").rotation_euler[0]=math.radians(90)
            cube("SET_Pipe_Header", (0, 2.5, 2.75), (7.2,.20,.20), m["trim"], bevel=.08)
        elif spec.builder == "apartment":
            cube("SET_KitchenIsland", (2.2, 1.3, .8), (3.0, 1.1, 1.6), m["cream"], bevel=.08)
            cube("PROP_Sofa_Base", (-2.2, .6, .45), (3.2, 1.15, .55), m["burgundy" if "burgundy" in m else "red"], kind="prop")
            cube("PROP_Sofa_Back", (-2.2, 1.05, 1.05), (3.2,.30,1.0), m["red"], kind="prop")
            cube("PROP_CoffeeTable", (-1.8,-1.2,.45), (2.2,1.1,.16), m["wood"], kind="prop")
            for x in (-3.6, 0):
                cube(f"SET_Window_{x}", (x, spec.depth/2-.12, 1.65), (1.5,.10,1.4), m["glass"], bevel=.04)
        else:
            cube("PROP_Bench", (0, .8, .45), (3.5,.75,.55), m["wood"], kind="prop")
            cyl("PROP_Plant", (-2.8, 1.7, .65), .45, 1.3, m["green"], vertices=14, kind="prop")


def build_outdoor(spec, m):
    w,d = spec.width,spec.depth
    if spec.builder in {"street", "street_corner", "intersection"}:
        cube("SET_Road", (0,0,-.10), (w,d,.20), m["road"], bevel=.03)
        if spec.builder == "street_corner":
            cube("SET_Road_East", (w*.30,d*.18,-.09), (w*.65,d*.42,.18), m["road"], bevel=.03)
        for x in (-w/2+.8,w/2-.8):
            cube(f"SET_Sidewalk_{'W' if x<0 else 'E'}", (x,0,.02), (1.6,d,.16), m["sidewalk"], bevel=.05)
        for i,y in enumerate(range(int(-d/2+2), int(d/2-1), 4)):
            cube(f"SET_RoadDash_{i}", (0,y,.025), (.18,1.8,.03), m["roadline"], bevel=.015)
        if spec.builder == "intersection":
            for i in range(-4,5):
                cube(f"SET_Crosswalk_{i+4}", (i*.8,-4.2,.03), (.45,3.0,.035), m["white"], bevel=.01)
    elif spec.builder in {"storefront", "store_shell", "business", "skyline"}:
        height = spec.height
        cube("SET_Building", (0,1.0,height/2), (w,d-2.0,height), m["brick" if spec.builder!="store_shell" else "cream"], bevel=.12)
        bays = max(2, int(w//3))
        for i in range(bays):
            x = -w/2 + (i+.5)*w/bays
            cube(f"SET_Window_{i}", (x,-d/2+.04,1.65), (w/bays*.65,.10,1.8), m["glass"], bevel=.04)
            cube(f"SET_Awning_{i}", (x,-d/2-.45,3.05), (w/bays*.82,.9,.18), m["red" if i%2==0 else "cream"], bevel=.06)
        door("Business", 0, -d/2+.02, m, width=1.5, height=2.45)
        cube("SET_SignBand", (0,-d/2-.12,3.75), (w*.72,.18,.72), m["blue" if spec.builder!="business" else "orange"], bevel=.08)
        if spec.builder == "store_shell":
            cube("SET_InteriorFloor", (0,0,-.01), (w*.9,d*.72,.14), m["floor"], bevel=.03)
            cube("PROP_Counter", (0,1.6,.85), (4.6,1.0,1.7), m["cream"], kind="interactable")
            for x in (-3,0,3):
                cube(f"PROP_Shelf_{x}", (x,2.9,1.2), (1.8,.45,2.4), m["wood"], kind="prop")
    else:
        cube("SET_Ground", (0,0,-.08), (w,d,.16), m["sidewalk" if spec.builder not in {"park","courtyard"} else "green"], bevel=.04)
        if spec.builder == "alley":
            for x in (-w/2,w/2):
                cube(f"SET_AlleyWall_{x}", (x,0,2.4), (.24,d,4.8), m["brick"], bevel=.06)
            cube("PROP_Dumpster", (1.4,2.6,.75), (2.2,1.1,1.5), m["green"], kind="interactable")
            for i in range(3):
                cyl(f"PROP_Bollard_{i}", (-1.8+i*1.8,-3.5,.5), .12,1.0,m["orange"],vertices=12,kind="prop")
        elif spec.builder == "bus_stop":
            cube("SET_ShelterRoof", (0,.3,2.5), (6.5,2.2,.18), m["blue"], bevel=.08)
            for x in (-3,3): cube(f"SET_ShelterPost_{x}",(x,.3,1.25),(.14,.14,2.5),m["metal"],bevel=.04)
            cube("SET_ShelterBack", (0,1.3,1.3), (6.0,.08,2.3), m["glass"], bevel=.03)
            cube("PROP_Bench", (0,.6,.55), (4.2,.65,.55), m["wood"], kind="interactable")
            cube("PROP_BusSign", (4.2,.2,1.9), (.7,.12,1.0), m["orange"], kind="interactable")
        elif spec.builder == "park":
            cube("SET_Path", (0,0,.02), (3.2,d,.06), m["sidewalk"], bevel=.05)
            for i,(x,y) in enumerate(((-5,-3),(5,-2),(-4,4),(4,4))):
                cyl(f"PROP_TreeTrunk_{i}",(x,y,1.3),.28,2.6,m["wood"],vertices=12,kind="prop")
                bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1,radius=1.5,location=(x,y,3.0)); o=bpy.context.object; o.name=f"PROP_TreeCrown_{i}"; o.data.materials.append(m["green"]); o["semantic_kind"]="prop"
            cube("PROP_ParkBench", (2.7,0,.48), (2.8,.65,.55), m["wood"], kind="interactable")
        elif spec.builder == "courtyard":
            cube("SET_PlazaInlay", (0,0,.02), (w*.68,d*.66,.05), m["cream"], bevel=.08)
            cyl("PROP_Fountain", (0,0,.45), 1.4,.9,m["blue"],vertices=24,kind="prop")
            for x in (-5,5): cube(f"PROP_Bench_{x}",(x,0,.45),(2.8,.65,.55),m["wood"],kind="prop")
        elif spec.builder == "parking":
            cube("SET_ParkingSurface", (0,0,.01), (w,d,.05), m["road"], bevel=.02)
            for i in range(6): cube(f"SET_ParkingLine_{i}",(-7.5+i*3,0,.04),(.10,d*.72,.025),m["white"],bevel=.01)
            cube("PROP_LoadingCrates", (5,3,.8), (2.4,2.4,1.6), m["wood"], kind="prop")
        for i,x in enumerate((-w*.35,w*.35)):
            cyl(f"SET_LampPost_{i}",(x,-d*.30,1.8),.09,3.6,m["metal"],vertices=12)
            cube(f"SET_Lamp_{i}",(x,-d*.30,3.55),(.65,.38,.20),m["cream"],bevel=.06)


def setup_preview(spec):
    scene=bpy.context.scene
    scene.render.engine="BLENDER_EEVEE"
    scene.render.resolution_x=480; scene.render.resolution_y=270; scene.render.resolution_percentage=100
    scene.render.image_settings.file_format="PNG"
    scene.world.color=(.025,.035,.055)
    cam_data=bpy.data.cameras.new("PREVIEW_Camera"); cam=bpy.data.objects.new("PREVIEW_Camera",cam_data); bpy.data.collections["PREVIEW"].objects.link(cam)
    distance=max(spec.width,spec.depth)*.95
    cam.location=(distance*.78,-distance*.88,max(4.4,spec.height*.82))
    cam.rotation_euler=(Vector((0,0,max(1.0,spec.height*.28)))-cam.location).to_track_quat("-Z","Y").to_euler(); cam_data.lens=45
    scene.camera=cam
    for name,loc,energy,size in (("PREVIEW_Key",(-4,-5,8),1200,5),("PREVIEW_Fill",(5,-1,5),650,4)):
        data=bpy.data.lights.new(name,"AREA"); data.energy=energy; data.shape="DISK"; data.size=size
        o=bpy.data.objects.new(name,data); bpy.data.collections["PREVIEW"].objects.link(o); o.location=loc; o.rotation_euler=(Vector((0,0,1))-o.location).to_track_quat("-Z","Y").to_euler()


def bevy_pos(v): return [round(float(v.x),5),round(float(v.z),5),round(float(-v.y),5)]


def record_objects(kind):
    out=[]
    for o in sorted(bpy.context.scene.objects,key=lambda x:x.name):
        if o.get("semantic_kind")!=kind: continue
        rec={"id":o.get("semantic_id",o.name),"node":o.name,"position":bevy_pos(o.location)}
        if o.type=="CAMERA": rec.update({"lens_mm":round(o.data.lens,2),"look_at":bevy_pos(Vector(o.get("look_at",[0,0,0])))})
        for k in ("socket_type","clearance_m","radius_m","interaction_type","collision_role"):
            if k in o: rec[k]=o[k]
        out.append(rec)
    return out


def build_module(spec: ModuleSpec):
    reset_scene(); m=materials(); scene=bpy.context.scene
    scene.unit_settings.system="METRIC"; scene.unit_settings.scale_length=1.0
    scene["module_id"]=spec.module_id; scene["module_version"]=1; scene["units"]="meters"; scene["forward_axis"]="-Z"; scene["up_axis"]="Y"
    if spec.kit=="apartment_building": build_indoor(spec,m)
    else: build_outdoor(spec,m)
    add_contract(spec); setup_preview(spec)
    source_dir=SOURCE_ROOT/spec.kit; runtime_dir=RUNTIME_ROOT/spec.kit; source_dir.mkdir(parents=True,exist_ok=True); runtime_dir.mkdir(parents=True,exist_ok=True); PREVIEW_ROOT.mkdir(parents=True,exist_ok=True)
    blend_path=source_dir/f"{spec.module_id}.blend"; glb_path=runtime_dir/f"{spec.module_id}.glb"; sidecar_path=runtime_dir/f"{spec.module_id}.module.json"; preview_path=PREVIEW_ROOT/f"{spec.module_id}.png"
    scene.render.filepath=str(preview_path); bpy.ops.wm.save_as_mainfile(filepath=str(blend_path)); bpy.ops.render.render(write_still=True)
    bpy.ops.object.select_all(action="DESELECT")
    for o in scene.objects:
        if not any(c.name=="PREVIEW" for c in o.users_collection): o.select_set(True)
    selected=[o for o in scene.objects if o.select_get()]
    bpy.context.view_layer.objects.active=selected[0]
    bpy.ops.export_scene.gltf(filepath=str(glb_path),check_existing=False,export_format="GLB",use_selection=True,export_yup=True,export_apply=True,export_cameras=True,export_lights=False,export_extras=True,export_animations=False,export_skins=False,export_morph=False,export_attributes=True,export_normals=True,export_texcoords=True,export_materials="EXPORT",export_image_format="AUTO",export_gpu_instances=False,export_loglevel=-1)
    digest=hashlib.sha256(glb_path.read_bytes()).hexdigest()
    sidecar={
        "schema_version":1,"module_id":spec.module_id,"asset":glb_path.relative_to(ROOT).as_posix(),"source_blend":blend_path.relative_to(ROOT).as_posix(),"category":spec.category,"version":1,
        "bounds":{"min":[-spec.width/2,0,-spec.depth/2],"max":[spec.width/2,spec.height,spec.depth/2]},
        "sockets":record_objects("socket"),"staging_marks":record_objects("staging_mark"),"camera_anchors":record_objects("camera_anchor"),"interactions":record_objects("interaction"),
        "cutaway_groups":record_objects("cutaway"),"collision_groups":record_objects("collider"),"tags":list(spec.tags),"glb_sha256":digest,"preview":preview_path.relative_to(ROOT).as_posix(),
        "provenance":{"author":"Infinite Backlot procedural Blender authoring","license":"repository license","generator":"tools/blender/build_world_kits.py"}
    }
    sidecar_path.write_text(json.dumps(sidecar,indent=2),encoding="utf-8")
    return sidecar


def main():
    records=[]
    for index,spec in enumerate(ALL_MODULES,1):
        print(f"[{index}/{len(ALL_MODULES)}] building {spec.module_id}")
        records.append(build_module(spec))
    registry={"schema_version":1,"registry_version":1,"coordinate_system":"Bevy right-handed Y-up","module_count":len(records),"modules":records}
    RUNTIME_ROOT.mkdir(parents=True,exist_ok=True); REGISTRY_PATH.write_text(json.dumps(registry,indent=2),encoding="utf-8")
    print(f"WORLD_KITS_COMPLETE modules={len(records)} registry={REGISTRY_PATH}")


if __name__ in {"__main__","builtins"}:
    main()
