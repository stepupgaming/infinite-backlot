"""Author the connected master's 2.5D navigation contract and Blender debug geometry.

Run with Blender 5.2 in background mode. Runtime coordinates are X/right, Y/up,
Z/forward; Blender helpers are converted to X/right, Y/-runtime-Z, Z/up.
"""
from __future__ import annotations

import json
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[2]
BLEND = ROOT / "assets/source/blender/world/neighborhood/infinite_backlot_block.blend"
OUT = ROOT / "assets/world/navigation/connected_navigation.json"
SIDECAR = ROOT / "assets/world/neighborhood/infinite_backlot_block.scene.json"

REGIONS = [
    {"id":"NAV_REGION_LOBBY","surface_type":"interior","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":40,"polygon":[[-4.0,-14.4],[4.0,-14.4],[4.0,-7.2],[-4.0,-7.2]],"connected_portals":["NAV_PORTAL_LOBBY_TO_ENTRANCE"]},
    {"id":"NAV_REGION_ENTRANCE","surface_type":"interior","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":50,"polygon":[[-1.35,-7.3],[1.35,-7.3],[1.35,-4.75],[-1.35,-4.75]],"connected_portals":["NAV_PORTAL_LOBBY_TO_ENTRANCE","NAV_PORTAL_BUILDING_ENTRANCE"]},
    {"id":"NAV_REGION_SIDEWALK","surface_type":"sidewalk","access":"public","height":0.0,"max_slope_deg":6.0,"actor_clearance":0.34,"priority":10,"polygon":[[-12.0,-6.65],[18.0,-6.65],[18.0,-3.65],[-12.0,-3.65]],"connected_portals":["NAV_PORTAL_BUILDING_ENTRANCE","NAV_PORTAL_TRANSIT_POCKET","NAV_PORTAL_ODD_HOURS_ENTRY"]},
    {"id":"NAV_REGION_TRANSIT_POCKET","surface_type":"interaction-only","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":60,"polygon":[[-5.4,-6.4],[0.2,-6.4],[0.2,-3.75],[-5.4,-3.75]],"connected_portals":["NAV_PORTAL_TRANSIT_POCKET"]},
    {"id":"NAV_REGION_STORE_VESTIBULE","surface_type":"interior","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":50,"polygon":[[16.15,-7.25],[17.9,-7.25],[17.9,-4.8],[16.15,-4.8]],"connected_portals":["NAV_PORTAL_ODD_HOURS_ENTRY","NAV_PORTAL_STORE_TO_AISLES"]},
    {"id":"NAV_REGION_ODD_HOURS_INTERIOR","surface_type":"interior","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":30,"polygon":[[13.0,-15.2],[22.2,-15.2],[22.2,-6.55],[13.0,-6.55]],"connected_portals":["NAV_PORTAL_STORE_TO_AISLES"]},
    {"id":"NAV_REGION_CROSSWALK","surface_type":"road","access":"public","height":0.0,"max_slope_deg":4.0,"actor_clearance":0.34,"priority":20,"polygon":[[13.2,-3.65],[16.8,-3.65],[16.8,3.0],[13.2,3.0]],"connected_portals":[]},
    {"id":"NAV_REGION_SERVICE_ALLEY","surface_type":"service","access":"private","height":0.0,"max_slope_deg":6.0,"actor_clearance":0.34,"priority":20,"polygon":[[-12.0,-22.0],[-7.8,-22.0],[-7.8,-7.2],[-12.0,-7.2]],"connected_portals":[]},
]

PORTALS = [
    {"id":"NAV_PORTAL_LOBBY_TO_ENTRANCE","regions":["NAV_REGION_LOBBY","NAV_REGION_ENTRANCE"],"position":[0.0,0.0,-7.25],"facing":[0.0,0.0,1.0],"width":1.8,"clearance":1.7,"traversal_type":"doorway","runtime_open":True,"control_entity":None},
    {"id":"NAV_PORTAL_BUILDING_ENTRANCE","regions":["NAV_REGION_ENTRANCE","NAV_REGION_SIDEWALK"],"position":[0.0,0.0,-5.05],"facing":[0.0,0.0,1.0],"width":1.65,"clearance":1.5,"traversal_type":"runtime-door","runtime_open":True,"control_entity":"CONTROL_MAIN_ENTRY"},
    {"id":"NAV_PORTAL_TRANSIT_POCKET","regions":["NAV_REGION_SIDEWALK","NAV_REGION_TRANSIT_POCKET"],"position":[-2.2,0.0,-4.25],"facing":[-1.0,0.0,0.0],"width":2.4,"clearance":2.2,"traversal_type":"open","runtime_open":True,"control_entity":None},
    {"id":"NAV_PORTAL_ODD_HOURS_ENTRY","regions":["NAV_REGION_SIDEWALK","NAV_REGION_STORE_VESTIBULE"],"position":[17.0,0.0,-5.3],"facing":[0.0,0.0,-1.0],"width":1.45,"clearance":1.3,"traversal_type":"runtime-door","runtime_open":True,"control_entity":"CONTROL_STORE_ENTRY"},
    {"id":"NAV_PORTAL_STORE_TO_AISLES","regions":["NAV_REGION_STORE_VESTIBULE","NAV_REGION_ODD_HOURS_INTERIOR"],"position":[17.0,0.0,-6.75],"facing":[0.0,0.0,-1.0],"width":1.45,"clearance":1.3,"traversal_type":"narrow-passage","runtime_open":True,"control_entity":None},
]

# Runtime-space axis-aligned box colliders: half extents are X/Y/Z.
def box(i, c, h, role="static"):
    return {"id":i,"shape":"box","center":c,"half_extents":h,"role":role}

COLLIDERS = [
    box("COLLIDER_LOBBY_WEST_WALL",[-4.15,1.4,-10.8],[0.15,1.4,3.7]),
    box("COLLIDER_LOBBY_EAST_WALL",[4.15,1.4,-10.8],[0.15,1.4,3.7]),
    box("COLLIDER_LOBBY_BACK_WALL",[0.0,1.4,-14.55],[4.3,1.4,0.15]),
    box("COLLIDER_LOBBY_FRONT_LEFT",[-2.7,1.4,-7.1],[1.35,1.4,0.15]),
    box("COLLIDER_LOBBY_FRONT_RIGHT",[2.7,1.4,-7.1],[1.35,1.4,0.15]),
    box("COLLIDER_FRONT_DESK",[2.15,0.65,-12.75],[1.35,0.65,0.65]),
    box("COLLIDER_MAILBOX_BANK",[-3.62,1.0,-11.4],[0.22,1.0,1.1]),
    box("COLLIDER_LOBBY_CHAIR_A",[1.9,0.5,-9.55],[0.52,0.5,0.48]),
    box("COLLIDER_LOBBY_CHAIR_B",[2.9,0.5,-9.55],[0.52,0.5,0.48]),
    box("COLLIDER_ENTRANCE_WEST",[-1.55,1.4,-6.0],[0.2,1.4,1.35]),
    box("COLLIDER_ENTRANCE_EAST",[1.55,1.4,-6.0],[0.2,1.4,1.35]),
    box("COLLIDER_TRANSIT_BENCH",[-3.05,0.55,-5.55],[1.1,0.55,0.42]),
    box("COLLIDER_TRANSIT_MAP",[-5.0,1.1,-5.9],[0.22,1.1,0.36]),
    box("COLLIDER_STREET_BOLLARD_A",[3.6,0.6,-5.65],[0.22,0.6,0.22]),
    box("COLLIDER_STREET_BOLLARD_B",[7.2,0.6,-5.65],[0.22,0.6,0.22]),
    box("COLLIDER_STREET_BOLLARD_C",[10.8,0.6,-5.65],[0.22,0.6,0.22]),
    box("COLLIDER_STORE_GLASS_LEFT",[14.55,1.35,-6.45],[1.55,1.35,0.12]),
    box("COLLIDER_STORE_GLASS_RIGHT",[20.15,1.35,-6.45],[2.05,1.35,0.12]),
    box("COLLIDER_STORE_WEST_WALL",[12.85,1.5,-10.9],[0.15,1.5,4.45]),
    box("COLLIDER_STORE_EAST_WALL",[22.35,1.5,-10.9],[0.15,1.5,4.45]),
    box("COLLIDER_STORE_BACK_WALL",[17.6,1.5,-15.35],[4.75,1.5,0.15]),
    box("COLLIDER_STORE_SHELF_A",[14.7,0.8,-10.0],[0.65,0.8,2.05]),
    box("COLLIDER_STORE_SHELF_B",[17.25,0.8,-11.05],[0.62,0.8,2.0]),
    box("COLLIDER_STORE_COOLERS",[20.65,1.1,-13.95],[1.35,1.1,0.55]),
    box("COLLIDER_STORE_COUNTER",[21.25,0.75,-9.75],[1.05,0.75,1.1]),
    box("COLLIDER_COUNTER_DISPLAY",[19.92,0.65,-10.35],[0.32,0.65,0.45]),
    box("COLLIDER_ALLEY_DUMPSTER",[-10.4,0.9,-18.2],[0.9,0.9,1.25]),
    box("COLLIDER_ALLEY_UTILITY",[-8.25,1.2,-13.0],[0.45,1.2,0.7]),
]

FLOOR_SUPPORTS = [
    {"id":f"FLOOR_SUPPORT_{r['id'][11:]}","region_id":r["id"],"height":r["height"],"polygon":r["polygon"]}
    for r in REGIONS
]

GUIDE_NODES = [
    {"id":"GUIDE_LOBBY_START","region_id":"NAV_REGION_LOBBY","position":[-2.7,0.0,-11.4]},
    {"id":"GUIDE_LOBBY_CLEAR","region_id":"NAV_REGION_LOBBY","position":[-1.75,0.0,-10.0]},
    {"id":"GUIDE_LOBBY_APPROACH","region_id":"NAV_REGION_LOBBY","position":[-0.85,0.0,-8.2]},
    {"id":"GUIDE_LOBBY_PORTAL","region_id":"NAV_REGION_LOBBY","portal_id":"NAV_PORTAL_LOBBY_TO_ENTRANCE","position":[0.0,0.0,-7.25]},
    {"id":"GUIDE_ENTRY_INNER","region_id":"NAV_REGION_ENTRANCE","position":[0.0,0.0,-6.35]},
    {"id":"GUIDE_BUILDING_PORTAL","region_id":"NAV_REGION_ENTRANCE","portal_id":"NAV_PORTAL_BUILDING_ENTRANCE","position":[0.0,0.0,-5.05]},
    {"id":"GUIDE_SIDEWALK_WEST","region_id":"NAV_REGION_SIDEWALK","position":[-0.25,0.0,-4.25]},
    {"id":"GUIDE_TRANSIT_WAIT","region_id":"NAV_REGION_TRANSIT_POCKET","portal_id":"NAV_PORTAL_TRANSIT_POCKET","position":[-2.2,0.0,-4.25]},
    {"id":"GUIDE_SIDEWALK_EAST_A","region_id":"NAV_REGION_SIDEWALK","position":[3.0,0.0,-4.25]},
    {"id":"GUIDE_SIDEWALK_EAST_B","region_id":"NAV_REGION_SIDEWALK","position":[8.0,0.0,-4.25]},
    {"id":"GUIDE_SIDEWALK_EAST_C","region_id":"NAV_REGION_SIDEWALK","position":[13.0,0.0,-4.35]},
    {"id":"GUIDE_STORE_PORTAL","region_id":"NAV_REGION_SIDEWALK","portal_id":"NAV_PORTAL_ODD_HOURS_ENTRY","position":[17.0,0.0,-5.3]},
    {"id":"GUIDE_STORE_VESTIBULE","region_id":"NAV_REGION_STORE_VESTIBULE","position":[17.0,0.0,-6.45]},
    {"id":"GUIDE_STORE_AISLE_PORTAL","region_id":"NAV_REGION_STORE_VESTIBULE","portal_id":"NAV_PORTAL_STORE_TO_AISLES","position":[17.0,0.0,-6.75]},
    {"id":"GUIDE_STORE_INSIDE_CLEAR","region_id":"NAV_REGION_ODD_HOURS_INTERIOR","position":[17.0,0.0,-7.25]},
    {"id":"GUIDE_STORE_AISLE_TURN","region_id":"NAV_REGION_ODD_HOURS_INTERIOR","position":[19.15,0.0,-7.25]},
    {"id":"GUIDE_COUNTER_APPROACH","region_id":"NAV_REGION_ODD_HOURS_INTERIOR","position":[19.55,0.0,-8.15]},
]
GUIDE_EDGES = [
    ["GUIDE_LOBBY_START","GUIDE_LOBBY_CLEAR"],["GUIDE_LOBBY_CLEAR","GUIDE_LOBBY_APPROACH"],["GUIDE_LOBBY_APPROACH","GUIDE_LOBBY_PORTAL"],
    ["GUIDE_LOBBY_PORTAL","GUIDE_ENTRY_INNER"],["GUIDE_ENTRY_INNER","GUIDE_BUILDING_PORTAL"],["GUIDE_BUILDING_PORTAL","GUIDE_SIDEWALK_WEST"],
    ["GUIDE_SIDEWALK_WEST","GUIDE_TRANSIT_WAIT"],["GUIDE_SIDEWALK_WEST","GUIDE_SIDEWALK_EAST_A"],["GUIDE_TRANSIT_WAIT","GUIDE_SIDEWALK_EAST_A"],
    ["GUIDE_SIDEWALK_EAST_A","GUIDE_SIDEWALK_EAST_B"],["GUIDE_SIDEWALK_EAST_B","GUIDE_SIDEWALK_EAST_C"],["GUIDE_SIDEWALK_EAST_C","GUIDE_STORE_PORTAL"],
    ["GUIDE_STORE_PORTAL","GUIDE_STORE_VESTIBULE"],["GUIDE_STORE_VESTIBULE","GUIDE_STORE_AISLE_PORTAL"],["GUIDE_STORE_AISLE_PORTAL","GUIDE_STORE_INSIDE_CLEAR"],["GUIDE_STORE_INSIDE_CLEAR","GUIDE_STORE_AISLE_TURN"],["GUIDE_STORE_AISLE_TURN","GUIDE_COUNTER_APPROACH"],
]

INTERACTION_VOLUMES = [
    {"id":"INTERACTION_VOLUME_TRANSIT_WAIT","interaction_id":"SMART_BUS_STOP_WAIT","center":[-2.2,0.9,-4.25],"half_extents":[0.65,0.9,0.65],"required_clearance":0.45},
    {"id":"INTERACTION_VOLUME_STORE_DOOR","interaction_id":"SMART_DOOR_OPEN","center":[17.0,1.0,-5.3],"half_extents":[0.8,1.0,0.8],"required_clearance":0.45},
    {"id":"INTERACTION_VOLUME_COUNTER_PICKUP","interaction_id":"SMART_PICKUP_SMALL","center":[19.55,1.0,-8.15],"half_extents":[0.65,1.0,0.65],"required_clearance":0.45},
    {"id":"INTERACTION_VOLUME_ELEVATOR_PANEL","interaction_id":"SMART_PANEL_PRESS","center":[1.15,1.1,-13.55],"half_extents":[0.6,1.0,0.6],"required_clearance":0.45},
]

CONTRACT = {
    "schema_version":1,"world_id":"infinite_backlot_block","coordinate_system":"bevy_y_up",
    "actor_defaults":{"capsule_radius":0.34,"capsule_half_height":0.82,"floor_sample_step":0.18,"path_sample_step":0.12,"turn_radius":0.55},
    "regions":REGIONS,"portals":PORTALS,"colliders":COLLIDERS,"floor_supports":FLOOR_SUPPORTS,
    "guide_nodes":GUIDE_NODES,"guide_edges":GUIDE_EDGES,"interaction_volumes":INTERACTION_VOLUMES,
    "route_proofs":[{"id":"ROUTE_MARA_LOBBY_TRANSIT_ODD_HOURS","start":[-2.7,0.0,-11.4],"destinations":[[-2.2,0.0,-4.25],[19.55,0.0,-8.15]],"arrival_heading":[1.0,0.0,0.0],"stop_distance":0.42}],
}


def runtime_to_blender(p):
    return (float(p[0]), -float(p[2]), float(p[1]))


def ensure_collection(name):
    old = bpy.data.collections.get(name)
    if old:
        for obj in list(old.objects): bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(old)
    col=bpy.data.collections.new(name); bpy.context.scene.collection.children.link(col); return col


def polygon_object(col, name, polygon, z, kind):
    verts=[runtime_to_blender([x,z,runtime_z]) for x,runtime_z in polygon]
    mesh=bpy.data.meshes.new(name+"_MESH"); mesh.from_pydata(verts,[],[list(range(len(verts)))]); mesh.update()
    obj=bpy.data.objects.new(name,mesh); col.objects.link(obj); obj.hide_render=True; obj.display_type='WIRE'; obj["semantic_type"]=kind; return obj


def cube_object(col, name, center, half_extents, kind):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=runtime_to_blender(center))
    obj=bpy.context.object; obj.name=name; obj.dimensions=(2*half_extents[0],2*half_extents[2],2*half_extents[1]); bpy.ops.object.transform_apply(location=False,rotation=False,scale=True)
    for parent in list(obj.users_collection): parent.objects.unlink(obj)
    col.objects.link(obj); obj.hide_render=True; obj.display_type='WIRE'; obj["semantic_type"]=kind; return obj


def main():
    if not BLEND.exists(): raise FileNotFoundError(BLEND)
    bpy.ops.wm.open_mainfile(filepath=str(BLEND))
    OUT.parent.mkdir(parents=True,exist_ok=True); OUT.write_text(json.dumps(CONTRACT,indent=2)+"\n",encoding="utf-8")
    col=ensure_collection("Backlot_Navigation")
    for r in REGIONS:
        obj=polygon_object(col,r["id"],r["polygon"],r["height"]+0.025,"walkable_region")
        for k in ["surface_type","access","max_slope_deg","actor_clearance","priority"]: obj[k]=r[k]
    for f in FLOOR_SUPPORTS:
        obj=polygon_object(col,f["id"],f["polygon"],f["height"],"floor_support"); obj["region_id"]=f["region_id"]
    for p in PORTALS:
        obj=cube_object(col,p["id"],p["position"],[p["width"]/2,1.0,0.06],"nav_portal")
        obj["regions"]=json.dumps(p["regions"]); obj["runtime_open"]=p["runtime_open"]; obj["traversal_type"]=p["traversal_type"]
        if p["control_entity"]: obj["control_entity"]=p["control_entity"]
    for c in COLLIDERS:
        obj=cube_object(col,c["id"],c["center"],c["half_extents"],"static_collider"); obj["collider_shape"]=c["shape"]; obj["collision_role"]=c["role"]
    for v in INTERACTION_VOLUMES:
        obj=cube_object(col,v["id"],v["center"],v["half_extents"],"interaction_volume"); obj["interaction_id"]=v["interaction_id"]
    for n in GUIDE_NODES:
        obj=cube_object(col,n["id"],n["position"],[0.08,0.08,0.08],"nav_lane_node"); obj["region_id"]=n["region_id"]
    side=json.loads(SIDECAR.read_text(encoding="utf-8")); side["navigation"]={"asset":"assets/world/navigation/connected_navigation.json","walkable_region_count":len(REGIONS),"portal_count":len(PORTALS),"collider_count":len(COLLIDERS),"floor_support_count":len(FLOOR_SUPPORTS),"interaction_volume_count":len(INTERACTION_VOLUMES)}; SIDECAR.write_text(json.dumps(side,indent=2)+"\n",encoding="utf-8")
    bpy.ops.wm.save_as_mainfile(filepath=str(BLEND),compress=True,relative_remap=True)
    print(json.dumps({"blend":str(BLEND),"navigation":str(OUT),"regions":len(REGIONS),"portals":len(PORTALS),"colliders":len(COLLIDERS),"floors":len(FLOOR_SUPPORTS),"guide_nodes":len(GUIDE_NODES)}))

if __name__ == "__main__": main()
