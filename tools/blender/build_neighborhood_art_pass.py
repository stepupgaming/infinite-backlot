"""Focused Infinite Backlot neighborhood environment-art pass.

This is intentionally a composition script, not a generic module factory. It upgrades
five named hero locations, preserves their runtime IDs, and builds one continuous
master block around the apartment entrance.
"""
from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

import bpy
from mathutils import Vector

ROOT=Path(r"C:/Projects/bevy-infinite")
KIT_BLEND=ROOT/"assets/source/blender/world/kits/infinite_backlot_detail_kit.blend"
KIT_CATALOG=ROOT/"assets/world/kits/infinite_backlot_detail_kit.catalog.json"
REGISTRY=ROOT/"assets/world/registry.json"
AFTER=ROOT/"assets/reference/world-art-pass/after"
MASTER_BLEND=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_block.blend"
MASTER_GLB=ROOT/"assets/world/neighborhood/infinite_backlot_block.glb"
MASTER_SCENE=ROOT/"assets/world/neighborhood/infinite_backlot_block.scene.json"
AFTER.mkdir(parents=True,exist_ok=True)

HERO_PATHS={
"apartment_exterior_a":(ROOT/"assets/source/blender/world/apartment_building/apartment_exterior_a.blend",ROOT/"assets/world/apartment_building/apartment_exterior_a.glb","building_exterior"),
"apartment_lobby_a":(ROOT/"assets/source/blender/world/apartment_building/apartment_lobby_a.blend",ROOT/"assets/world/apartment_building/apartment_lobby_a.glb","lobby"),
"neighborhood_intersection_a":(ROOT/"assets/source/blender/world/neighborhood/neighborhood_intersection_a.blend",ROOT/"assets/world/neighborhood/neighborhood_intersection_a.glb","intersection"),
"neighborhood_convenience_store_a":(ROOT/"assets/source/blender/world/neighborhood/neighborhood_convenience_store_a.blend",ROOT/"assets/world/neighborhood/neighborhood_convenience_store_a.glb","hero_business"),
"neighborhood_alley_a":(ROOT/"assets/source/blender/world/neighborhood/neighborhood_alley_a.blend",ROOT/"assets/world/neighborhood/neighborhood_alley_a.glb","alley"),
}

COLORS={
"MAT_PLASTER_WARM":((0.47,0.34,0.24,1),.82,0),"MAT_PLASTER_CREAM":((.72,.58,.39,1),.78,0),
"MAT_BRICK_RED":((.38,.055,.025,1),.88,0),"MAT_BRICK_DARK":((.20,.025,.018,1),.92,0),
"MAT_CONCRETE":((.30,.31,.29,1),.92,0),"MAT_SIDEWALK":((.43,.44,.39,1),.86,0),
"MAT_ASPHALT":((.028,.035,.045,1),.96,0),"MAT_PAINT_TEAL":((.025,.18,.19,1),.52,.05),
"MAT_PAINT_CREAM":((.82,.67,.42,1),.65,0),"MAT_PAINT_BURGUNDY":((.31,.025,.055,1),.58,0),
"MAT_METAL_DARK":((.07,.08,.09,1),.36,.72),"MAT_METAL_GALVANIZED":((.36,.40,.42,1),.31,.62),
"MAT_BRASS":((.58,.32,.07,1),.25,.76),"MAT_WOOD":((.24,.075,.025,1),.68,0),
"MAT_PLASTIC_RED":((.62,.025,.018,1),.42,0),"MAT_PLASTIC_BLUE":((.025,.16,.42,1),.42,0),
"MAT_RUBBER":((.018,.022,.025,1),.94,0),"MAT_TILE_GREEN":((.05,.28,.21,1),.34,0),
"MAT_CARPET":((.15,.16,.13,1),.98,0),"MAT_GLASS":((.025,.16,.23,.42),.12,.08),
"MAT_EMISSIVE_WARM":((1.0,.36,.06,1),.25,0),"MAT_EMISSIVE_CYAN":((.04,.72,.82,1),.22,0),
"MAT_PAPER":((.82,.75,.57,1),.90,0),"MAT_GRIME":((.055,.035,.025,1),.96,0),
"MAT_GREEN":((.03,.26,.09,1),.82,0),"MAT_CARDBOARD":((.48,.27,.10,1),.90,0),
}


def reset():
    bpy.ops.object.select_all(action="SELECT");bpy.ops.object.delete(use_global=False)
    for data in (bpy.data.meshes,bpy.data.curves,bpy.data.materials,bpy.data.cameras,bpy.data.lights):
        for item in list(data):data.remove(item)
    for c in list(bpy.data.collections):bpy.data.collections.remove(c)
    scene=bpy.context.scene;scene.unit_settings.system="METRIC";scene.render.engine="BLENDER_EEVEE";scene.world.color=(.018,.025,.035)
    scene.render.resolution_x=720;scene.render.resolution_y=405;scene.render.resolution_percentage=100
    for name in ("ARCHITECTURE","DRESSING","RUNTIME_OBJECTS","SEMANTICS","CAMERAS","COLLIDERS","CUTAWAYS","LIGHTING"):
        scene.collection.children.link(bpy.data.collections.new(name))


def material(name):
    found=bpy.data.materials.get(name)
    if found:return found
    color,rough,metal=COLORS[name];m=bpy.data.materials.new(name);m.diffuse_color=color;m.use_nodes=True;p=m.node_tree.nodes.get("Principled BSDF");p.inputs["Base Color"].default_value=color;p.inputs["Roughness"].default_value=rough;p.inputs["Metallic"].default_value=metal
    if name=="MAT_GLASS":p.inputs["Alpha"].default_value=.42;p.inputs["Transmission Weight"].default_value=.18;m.surface_render_method="DITHERED"
    if name.startswith("MAT_EMISSIVE"):p.inputs["Emission Color"].default_value=color;p.inputs["Emission Strength"].default_value=4.2
    m["gltf_compatible"]=True;m["shared_library"]="infinite_backlot_v2";return m


def move(obj,collection):
    for c in list(obj.users_collection):c.objects.unlink(obj)
    bpy.data.collections[collection].objects.link(obj)


def cube(name,loc,dims,mat="MAT_PLASTER_WARM",bevel=.05,collection="ARCHITECTURE",rot=(0,0,0),kind="static"):
    bpy.ops.mesh.primitive_cube_add(location=loc,rotation=rot);o=bpy.context.object;o.name=name;o.dimensions=dims;bpy.ops.object.transform_apply(location=False,rotation=False,scale=True);o.data.materials.append(material(mat));move(o,collection)
    if bevel:
        mod=o.modifiers.new("Art bevel","BEVEL");mod.width=min(bevel,min(dims)*.22);mod.segments=2
    o["semantic_kind"]=kind;return o


def cyl(name,loc,r,depth,mat="MAT_METAL_DARK",vertices=16,collection="DRESSING",rot=(0,0,0),kind="static"):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=r,depth=depth,location=loc,rotation=rot);o=bpy.context.object;o.name=name;o.data.materials.append(material(mat));move(o,collection);o["semantic_kind"]=kind;return o


def sphere(name,loc,scale,mat,collection="DRESSING"):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1,radius=1,location=loc);o=bpy.context.object;o.name=name;o.scale=scale;o.data.materials.append(material(mat));move(o,collection);return o


def text_mesh(name,text,loc,size=.42,mat="MAT_PAPER",rot=(math.pi/2,0,0),align="CENTER",collection="DRESSING",extrude=.012):
    curve=bpy.data.curves.new(name,"FONT");curve.body=text;curve.align_x=align;curve.align_y="CENTER";curve.size=size;curve.extrude=extrude;obj=bpy.data.objects.new(name,curve);bpy.data.collections[collection].objects.link(obj);obj.location=loc;obj.rotation_euler=rot;obj.data.materials.append(material(mat));bpy.context.view_layer.objects.active=obj;obj.select_set(True);bpy.ops.object.convert(target="MESH");obj.select_set(False);return obj


def semantic(name,loc,kind,**props):
    o=bpy.data.objects.new(name,None);bpy.data.collections["SEMANTICS"].objects.link(o);o.location=loc;o.empty_display_type="ARROWS" if kind=="socket" else "CIRCLE";o.empty_display_size=.3;o["semantic_kind"]=kind;o["semantic_id"]=name
    for k,v in props.items():o[k]=v
    return o


def camera(name,loc,target,lens=42):
    d=bpy.data.cameras.new(name);d.lens=lens;d.sensor_width=36;o=bpy.data.objects.new(name,d);bpy.data.collections["CAMERAS"].objects.link(o);o.location=loc;o.rotation_euler=(Vector(target)-o.location).to_track_quat("-Z","Y").to_euler();o["semantic_kind"]="camera_anchor";o["semantic_id"]=name;o["look_at"]=list(target);return o


def collider(name,loc,dims):
    o=cube(name,loc,dims,"MAT_RUBBER",0,"COLLIDERS",kind="collider");o.hide_render=True;o.display_type="WIRE";o["collision_role"]="solid";return o


def area_light(name,loc,target,energy=500,size=3,color=(1,.72,.48)):
    d=bpy.data.lights.new(name,"AREA");d.energy=energy;d.shape="DISK";d.size=size;d.color=color;o=bpy.data.objects.new(name,d);bpy.data.collections["LIGHTING"].objects.link(o);o.location=loc;o.rotation_euler=(Vector(target)-o.location).to_track_quat("-Z","Y").to_euler();return o


def sun():
    d=bpy.data.lights.new("ART_Sun","SUN");d.energy=2.0;d.angle=math.radians(22);d.color=(1,.72,.50);o=bpy.data.objects.new("ART_Sun",d);bpy.data.collections["LIGHTING"].objects.link(o);o.rotation_euler=(math.radians(34),math.radians(-18),math.radians(28))


def load_kit():
    wanted=["KIT_"+x["asset_id"].upper() for x in json.loads(KIT_CATALOG.read_text())["assets"]]
    with bpy.data.libraries.load(str(KIT_BLEND),link=True) as (src,dst):dst.collections=[n for n in wanted if n in src.collections]


def kit(asset_id,loc,rot=(0,0,0),scale=(1,1,1),name=None):
    coll=bpy.data.collections.get("KIT_"+asset_id.upper());o=bpy.data.objects.new(name or "PROP_"+asset_id,None);bpy.data.collections["DRESSING"].objects.link(o);o.instance_type="COLLECTION";o.instance_collection=coll;o.location=loc;o.rotation_euler=rot;o.scale=scale;o["kit_asset_id"]=asset_id;return o


def brick_band(prefix,y,z,width=14,x0=0):
    # Deliberate masonry breakup: alternating darker brick strips and recessed joints.
    for row in range(5):
        zz=z+row*.42
        for col in range(14):
            xx=x0-width/2+.28+col*(width/14);offset=(width/28 if row%2 else 0)
            cube(f"{prefix}_Brick_{row}_{col}",(xx+offset,y,zz),(width/14-.045,.09,.34),"MAT_BRICK_DARK" if (row*7+col)%9==0 else "MAT_BRICK_RED",.025,"ARCHITECTURE")


def add_window_facade(x,y,z,double=False):
    kit("window_double" if double else "window_single",(x,y,z),rot=(math.pi/2,0,0))
    cube("WindowRecess",(x,y+.08,z+.65),((2.55 if double else 1.35),.18,1.45),"MAT_GRIME",.02)


def build_exterior(origin=(0,0,0),master=False):
    ox,oy,oz=origin;front=oy-6;back=oy+6
    # Layered silhouette: ground plinth, recessed entrance, upper setbacks, roof crown.
    cube("Exterior_Plinth",(ox,oy,oz+.35),(14,12,.7),"MAT_BRICK_DARK",.08)
    cube("Exterior_UpperMass",(ox,oy+.25,oz+7.5),(14,11.5,8.4),"MAT_BRICK_RED",.12)
    cube("Exterior_RoofSetback",(ox+.55,oy+.7,oz+11.9),(10.8,9.4,1.1),"MAT_PLASTER_WARM",.12)
    cube("Facade_Left",(ox-4.35,front,oz+2.3),(5.3,.38,3.9),"MAT_BRICK_RED",.06);cube("Facade_Right",(ox+4.35,front,oz+2.3),(5.3,.38,3.9),"MAT_BRICK_RED",.06)
    cube("EntranceRevealLeft",(ox-1.55,front+.36,oz+1.75),(.30,.32,3.5),"MAT_GRIME",.04);cube("EntranceRevealRight",(ox+1.55,front+.36,oz+1.75),(.30,.32,3.5),"MAT_GRIME",.04);cube("EntranceRevealHeader",(ox,front+.36,oz+3.35),(3.4,.32,.30),"MAT_GRIME",.04)
    # Pilasters and string courses produce hierarchy instead of one slab.
    for x in (-6.55,-3.55,3.55,6.55):cube("Facade_Pilaster",(ox+x,front-.12,oz+7.0),(.34,.36,10.5),"MAT_BRICK_DARK",.04)
    for z in (4.0,7.25,10.45):cube("Facade_Stringcourse",(ox,front-.16,oz+z),(14.2,.34,.22),"MAT_PAINT_CREAM",.04)
    brick_band("Facade",front-.23,oz+.55,14,ox)
    for floor,z in enumerate((5.15,8.35)):
        for x in (-4.9,-1.7,1.7,4.9):add_window_facade(ox+x,front-.34,oz+z,double=(floor==0 and x in (-1.7,1.7)))
    # Side volume and offset service bay make the silhouette asymmetrical.
    cube("ServiceBay",(ox-7.0,oy+2.2,oz+3.1),(2.0,5.6,6.2),"MAT_BRICK_DARK",.10)
    kit("entry_door",(ox,front-.24,oz),rot=(math.pi/2,0,0));kit("canopy",(ox,front-1.0,oz),rot=(math.pi/2,0,0),scale=(1.18,1.15,1.0))
    kit("wall_light",(ox-2.0,front-.32,oz+1.35),rot=(math.pi/2,0,0));kit("wall_light",(ox+2.0,front-.32,oz+1.35),rot=(math.pi/2,0,0))
    kit("fire_escape",(ox-7.15,oy+1.0,oz+.2),rot=(0,0,math.pi/2));kit("drainpipe",(ox+6.35,front-.3,oz),rot=(0,0,0))
    for i,(x,z) in enumerate(((-5.0,5.1),(-1.7,8.3),(4.9,5.1))):kit("ac_unit",(ox+x,front-.78,oz+z-.55),rot=(math.pi/2,0,0),scale=(.62,.62,.62))
    kit("utility_box",(ox-5.8,front-.5,oz+.2),rot=(math.pi/2,0,0),scale=(.7,.7,.7))
    # Rooftop silhouette.
    for x in (-3.8,0,3.4):cyl("RoofVent",(ox+x,oy+.4,oz+12.9),.28,1.5,"MAT_METAL_GALVANIZED",14)
    cyl("WaterTank",(ox-2.5,oy+1.4,oz+13.5),1.05,2.1,"MAT_METAL_DARK",16);cyl("TankCap",(ox-2.5,oy+1.4,oz+14.6),1.12,.18,"MAT_PAINT_TEAL",16)
    cube("RoofSignFrame",(ox+3.2,front+1.0,oz+13.6),(4.3,.18,1.8),"MAT_METAL_DARK",.04);text_mesh("AddressSign","314 1/2",(ox,front-.48,oz+3.25),.42,"MAT_EMISSIVE_WARM")
    text_mesh("RoofNotice","INFINITE BACKLOT",(ox+3.2,front+.88,oz+13.6),.48,"MAT_EMISSIVE_CYAN")
    cube("GroundGrime",(ox,front-.23,oz+.18),(13.8,.05,.34),"MAT_GRIME",.01)
    # Entry apron and steps establish human scale.
    for i in range(3):cube("EntranceStep",(ox,front-1.10-i*.38,oz+.08+i*.09),(4.2-i*.25,.72,.16),"MAT_CONCRETE",.05)
    semantic("SOCKET_EXTERIOR",(ox,front-2.3,oz),"socket",socket_type="EXTERIOR",clearance_m=2.2);semantic("SOCKET_ROOFTOP",(ox,oy,oz+13),"socket",socket_type="ROOFTOP",clearance_m=1.5)
    semantic("MARK_BUILDING_ENTRY",(ox,front-1.9,oz),"staging_mark",radius_m=.65);semantic("MARK_EXTERIOR_WAIT",(ox+3.0,front-1.9,oz),"staging_mark",radius_m=.6);semantic("MARK_EXTERIOR_TWO_SHOT_A",(ox-1.2,front-2.7,oz),"staging_mark",radius_m=.6);semantic("MARK_EXTERIOR_TWO_SHOT_B",(ox+1.2,front-2.7,oz),"staging_mark",radius_m=.6)
    semantic("INTERACT_MAIN_DOOR",(ox,front-.6,oz+1),"interaction",interaction_type="door");camera("CAM_STREET_BUILDING_ESTABLISH",(ox+10,front-14,oz+6.4),(ox,front,oz+5),42);camera("CAM_ENTRANCE_LOW_WIDE",(ox-5.2,front-7.3,oz+1.65),(ox,front,oz+2.2),34)
    collider("COLLIDER_EXTERIOR_FOOTPRINT",(ox,oy,oz-.20),(14,12,.3))


def build_lobby(origin=(0,0,0),master=False):
    ox,oy,oz=origin;front=oy-4;back=oy+4
    cube("LobbyFloor",(ox,oy,oz-.08),(8,8,.16),"MAT_TILE_GREEN",.02)
    # Offset tile bands and brass inlays create a recognizable floor rhythm.
    for x in (-2.8,-1.4,0,1.4,2.8):cube("LobbyTileInlay",(ox+x,oy,oz+.015),(.055,7.8,.028),"MAT_BRASS",.005)
    for y in (-2.6,-1.3,0,1.3,2.6):cube("LobbyTileInlay",(ox,oy+y,oz+.018),(7.8,.055,.03),"MAT_PAINT_CREAM",.005)
    cube("LobbyWallLeft",(ox-4,oy,oz+2.1),(.20,8,4.2),"MAT_PLASTER_CREAM",.04);cube("LobbyWallRight",(ox+4,oy,oz+2.1),(.20,8,4.2),"MAT_PLASTER_CREAM",.04);cube("LobbyBackWall",(ox,back,oz+2.1),(8,.20,4.2),"MAT_PLASTER_CREAM",.04)
    for x in (-3.86,3.86):cube("LobbyWainscot",(ox+x,oy,oz+.65),(.09,7.7,1.25),"MAT_PAINT_TEAL",.02)
    cube("LobbyCrownBack",(ox,back-.10,oz+3.75),(7.8,.16,.34),"MAT_BRASS",.03)
    # Recessed elevator portal at the actual rear of the lobby.
    cube("ElevatorPortal",(ox,back-.18,oz+1.7),(3.4,.32,3.4),"MAT_METAL_DARK",.06);cube("ElevatorDoorL",(ox-.67,back-.39,oz+1.55),(1.28,.08,2.9),"MAT_METAL_GALVANIZED",.025,"RUNTIME_OBJECTS",kind="runtime_controlled");cube("ElevatorDoorR",(ox+.67,back-.39,oz+1.55),(1.28,.08,2.9),"MAT_METAL_GALVANIZED",.025,"RUNTIME_OBJECTS",kind="runtime_controlled")
    cube("ElevatorIndicator",(ox,back-.47,oz+3.35),(1.2,.08,.34),"MAT_EMISSIVE_CYAN",.04,"RUNTIME_OBJECTS",kind="runtime_controlled");text_mesh("ElevatorLetter","?",(ox,back-.53,oz+3.35),.28,"MAT_PAPER")
    kit("mailbox_bank",(ox-3.62,oy+.4,oz+.05),rot=(0,0,-math.pi/2),scale=(.92,.92,.92));kit("notice_board",(ox+3.68,oy-.55,oz),rot=(0,0,math.pi/2))
    kit("checkout_counter",(ox+2.5,oy+2.15,oz),rot=(0,0,math.pi/2),scale=(.72,.72,.72),name="PROP_ReceptionDesk")
    for p,r in [((-2.3,oy-1.5,oz),0),((-1.35,oy-1.5,oz),0),((-2.3,oy-.45,oz),math.pi)]:kit("lobby_chair",(ox+p[0],p[1],p[2]),rot=(0,0,r),scale=(.86,.86,.86))
    kit("potted_plant",(ox+3.2,front+.8,oz),scale=(.72,.72,.72));kit("delivery_box",(ox+2.9,oy+1.2,oz),rot=(0,0,.18));kit("delivery_box",(ox+3.35,oy+1.35,oz),rot=(0,0,-.12),scale=(.7,.7,.7))
    # Bureaucratic storytelling signs.
    text_mesh("LobbyRules","LOBBY RULE 8B:\nNO PORTALS AFTER 9 PM",(ox+3.82,oy-2.1,oz+2.15),.18,"MAT_PAPER",rot=(math.pi/2,0,math.pi/2),align="CENTER")
    text_mesh("InspectionNotice","ELEVATOR CERTIFIED\nPROVISIONALLY",(ox+2.65,back-.20,oz+2.1),.15,"MAT_PAPER")
    # Ceiling coffers and practical fixtures.
    cube("LobbyCeiling",(ox,oy,oz+4.15),(8,8,.18),"MAT_PLASTER_WARM",.03)
    for y in (-2.2,0,2.2):cube("LobbyFixture",(ox,oy+y,oz+3.95),(2.0,.45,.08),"MAT_EMISSIVE_WARM",.08,"RUNTIME_OBJECTS",kind="runtime_controlled");area_light("LobbyPractical",(ox,oy+y,oz+3.72),(ox,oy+y,oz),260,2.2)
    # Front glass doors preserve exterior sightline in the master.
    if not master:
        cube("CUTAWAY_FRONT",(ox,front,oz+2.1),(8,.15,4.2),"MAT_PLASTER_CREAM",.02,"CUTAWAYS",kind="cutaway").hide_render=True
    semantic("SOCKET_HALL_SOUTH",(ox,front,oz),"socket",socket_type="HALL",clearance_m=2);semantic("SOCKET_ELEVATOR",(ox,back,oz),"socket",socket_type="ELEVATOR",clearance_m=2);semantic("SOCKET_HALL_NORTH",(ox,back,oz),"socket",socket_type="HALL",clearance_m=2)
    for name,loc in [("MARK_LOBBY_MAILBOX",(ox-2.7,oy+.4,oz)),("MARK_FRONT_DESK",(ox+1.45,oy+2.1,oz)),("MARK_LOBBY_CONVERSATION_A",(ox-.7,oy-.25,oz)),("MARK_LOBBY_CONVERSATION_B",(ox+.7,oy-.25,oz)),("MARK_ELEVATOR_WAIT",(ox,back-1.3,oz))]:semantic(name,loc,"staging_mark",radius_m=.58)
    semantic("INTERACT_DIRECTORY",(ox+3.55,oy-.55,oz+1.2),"interaction",interaction_type="directory");semantic("INTERACT_MAILBOXES",(ox-3.5,oy+.4,oz+1.1),"interaction",interaction_type="mailboxes");semantic("INTERACT_ELEVATOR_PANEL",(ox+1.65,back-.44,oz+1.15),"interaction",interaction_type="panel")
    camera("CAM_LOBBY_ENTRANCE_WIDE",(ox,front+1.0,oz+1.7),(ox,oy+.8,oz+1.35),38);camera("CAM_ELEVATOR_FROM_MAILBOXES",(ox-3.05,oy+.25,oz+1.65),(ox,back-.4,oz+1.35),48);camera("CAM_MAILBOX_TWO_SHOT",(ox+1.8,oy-1.5,oz+1.65),(ox-2.7,oy+.25,oz+1.15),54)
    collider("COLLIDER_LOBBY_FLOOR",(ox,oy,oz-.20),(8,8,.3))


def build_intersection(origin=(0,0,0),master=False):
    ox,oy,oz=origin
    cube("RoadMain",(ox,oy,oz-.12),(34,9,.24),"MAT_ASPHALT",.02);cube("RoadCross",(ox+10,oy+5,oz-.11),(9,24,.24),"MAT_ASPHALT",.02)
    # Four raised sidewalk quadrants, curb cuts, storm drains, and road wear.
    quadrants=[(-10,6,12,4),(0,6,6,4),(-10,-6,12,4),(0,-6,6,4),(16,6,8,4),(16,-6,8,4)]
    for i,(x,y,w,d) in enumerate(quadrants):cube(f"Sidewalk_{i}",(ox+x,oy+y,oz+.08),(w,d,.22),"MAT_SIDEWALK",.04)
    for x in (-14,-8,-2,4,16):cube("Curb",(ox+x,oy+4.58,oz+.12),(5.5,.24,.34),"MAT_CONCRETE",.04)
    for i in range(10):cube("CrosswalkMain",(ox+7.2+i*.62,oy,oz+.02),(.34,6.6,.025),"MAT_PAINT_CREAM",.01)
    for i in range(7):cube("CrosswalkSide",(ox+10,oy+6.1+i*.62,oz+.02),(6.6,.34,.025),"MAT_PAINT_CREAM",.01)
    for x in (-12,-5,2):cube("LaneDash",(ox+x,oy,oz+.015),(2.1,.12,.025),"MAT_PAINT_CREAM",.01)
    cube("RoadPatch",(ox-4,oy-1.5,oz+.005),(5.5,1.8,.02),"MAT_GRIME",.01,rot=(0,0,.07))
    for x in (-10,2,16):kit("street_lamp",(ox+x,oy+5.0,oz),scale=(.9,.9,.9));kit("storm_drain",(ox+x+1.4,oy+4.2,oz+.02),scale=(.8,.8,.8))
    kit("hydrant",(ox-3.3,oy+5.5,oz),scale=(.85,.85,.85));kit("bench",(ox-8.0,oy+6.0,oz),rot=(0,0,math.pi));kit("planter",(ox-11.0,oy+6.1,oz),scale=(.72,.72,.72));kit("bike_rack",(ox+2.0,oy+6.0,oz),rot=(0,0,math.pi/2),scale=(.75,.75,.75))
    for p in ((ox+5.7,oy+4.8,oz),(ox+14.5,oy+4.8,oz)):kit("traffic_cone",p,scale=(.8,.8,.8))
    # Corner sign pole and layered background façades.
    cyl("StreetSignPole",(ox+5.4,oy+5.0,oz+1.4),.08,2.8,"MAT_METAL_DARK",12);cube("StreetSignA",(ox+5.4,oy+4.93,oz+2.7),(1.8,.08,.38),"MAT_PAINT_TEAL",.04);text_mesh("StreetName","BACKLOT AVE",(ox+5.4,oy+4.87,oz+2.7),.16,"MAT_PAPER")
    for i,(x,w,h,matn) in enumerate(((-14,8,8,"MAT_BRICK_DARK"),(-5,7,6.5,"MAT_PLASTER_WARM"),(15,9,8.5,"MAT_PAINT_TEAL"))):
        cube(f"BackgroundFacade_{i}",(ox+x,oy+10,oz+h/2),(w,3,h),matn,.10)
        for row in range(2):
            for col in range(max(2,int(w//2.5))):cube("BgWindow",(ox+x-w/2+1.1+col*2.2,oy+8.45,oz+2.4+row*2.5),(1.1,.06,1.2),"MAT_GLASS",.02)
    semantic("ROAD_EAST",(ox+17,oy,oz),"socket",socket_type="ROAD",clearance_m=4);semantic("ROAD_WEST",(ox-17,oy,oz),"socket",socket_type="ROAD",clearance_m=4);semantic("ROAD_NORTH",(ox+10,oy+12,oz),"socket",socket_type="ROAD",clearance_m=4);semantic("ROAD_SOUTH",(ox+10,oy-12,oz),"socket",socket_type="ROAD",clearance_m=4)
    for name,loc in [("MARK_CROSSWALK",(ox+10,oy,oz)),("MARK_STREET_CORNER_TWO_SHOT_A",(ox+2.2,oy+5.6,oz+.2)),("MARK_STREET_CORNER_TWO_SHOT_B",(ox+3.5,oy+5.6,oz+.2)),("MARK_BUS_STOP_BENCH",(ox-8,oy+5.5,oz+.2))]:semantic(name,loc,"staging_mark",radius_m=.65)
    camera("CAM_STREET_BUILDING_ESTABLISH",(ox+16,oy-15,oz+5.8),(ox-2,oy+7,oz+3),48);camera("CAM_SIDEWALK_TWO_SHOT",(ox-2,oy+2.5,oz+1.65),(ox+2.8,oy+5.6,oz+1.15),56);camera("CAM_CROSSWALK_LOW",(ox+14,oy-7,oz+1.4),(ox+9,oy,oz+.8),38)
    collider("COLLIDER_STREET",(ox,oy,oz-.25),(34,9,.35))


def build_store(origin=(0,0,0),master=False):
    ox,oy,oz=origin;front=oy-4.5;back=oy+4.5
    cube("StoreFloor",(ox,oy,oz-.08),(11,9,.16),"MAT_TILE_GREEN",.02);cube("StoreBackWall",(ox,back,oz+2.25),(11,.20,4.5),"MAT_PLASTER_CREAM",.04);cube("StoreLeftWall",(ox-5.5,oy,oz+2.25),(.20,9,4.5),"MAT_BRICK_DARK",.04);cube("StoreRightWall",(ox+5.5,oy,oz+2.25),(.20,9,4.5),"MAT_BRICK_RED",.04)
    cube("StoreFacadeHeader",(ox,front,oz+3.85),(11,.30,1.3),"MAT_PAINT_TEAL",.08);cube("StoreFacadeBase",(ox,front,oz+.42),(11,.30,.84),"MAT_BRICK_DARK",.05)
    # Full glass frontage with real mullions and recessed entrance.
    for x in (-4.2,-2.8,-1.4,1.4,2.8,4.2):cube("StoreMullion",(ox+x,front-.16,oz+2.05),(.11,.14,2.8),"MAT_BRASS",.02)
    for x in (-3.5,-2.1,2.1,3.5):cube("StoreGlass",(ox+x,front-.08,oz+2.05),(1.25,.04,2.65),"MAT_GLASS",.01)
    kit("entry_door",(ox,front-.22,oz),rot=(math.pi/2,0,0),scale=(1.0,1.0,1.25));kit("awning",(ox,front-.9,oz+1.25),rot=(math.pi/2,0,0),scale=(2.7,1.0,1.0))
    text_mesh("StoreSign","ODD HOURS",(ox,front-.25,oz+4.03),.62,"MAT_EMISSIVE_CYAN")
    text_mesh("WindowAdA","OPEN-ISH",(ox-3.5,front-.22,oz+2.0),.24,"MAT_PAPER",rot=(math.pi/2,0,.05));text_mesh("WindowAdB","BUY 2\nREMEMBER 1",(ox+3.5,front-.22,oz+2.0),.18,"MAT_EMISSIVE_WARM",rot=(math.pi/2,0,-.06))
    # Three readable aisles, refrigerators, counter, and back-room relationship.
    for x in (-2.0,0,2.0):kit("store_shelf_stocked",(ox+x,oy+.1,oz),rot=(0,0,math.pi/2),scale=(.78,.78,.78))
    kit("store_fridge_stocked",(ox,back-.35,oz),scale=(1.15,1.0,1.0));kit("checkout_counter",(ox+3.8,oy-2.4,oz),rot=(0,0,math.pi/2),scale=(.82,.82,.82))
    kit("service_door",(ox-3.9,back-.15,oz),rot=(math.pi/2,0,0),scale=(.85,.85,.85));kit("delivery_box",(ox-4.5,back-1.0,oz),rot=(0,0,.12));kit("delivery_box",(ox-3.9,back-1.1,oz),rot=(0,0,-.08),scale=(.7,.7,.7));kit("potted_plant",(ox+4.7,front+.9,oz),scale=(.55,.55,.55))
    cube("StoreCeiling",(ox,oy,oz+4.45),(11,9,.16),"MAT_PLASTER_WARM",.03)
    for x in (-3,0,3):cube("StoreFixture",(ox+x,oy,oz+4.22),(.45,3.0,.08),"MAT_EMISSIVE_CYAN",.06,"RUNTIME_OBJECTS",kind="runtime_controlled");area_light("StorePractical",(ox+x,oy,oz+4.0),(ox+x,oy,oz),280,2.4,(.55,.85,1.0))
    semantic("SIDEWALK_SOUTH",(ox,front-1.4,oz),"socket",socket_type="SIDEWALK",clearance_m=2);semantic("BUILDING_ENTRANCE_MAIN",(ox,front,oz),"socket",socket_type="ENTRANCE",clearance_m=1.5);semantic("ALLEY_NORTH",(ox-5.5,back,oz),"socket",socket_type="ALLEY",clearance_m=1.5)
    for name,loc in [("MARK_STORE_ENTRANCE",(ox,front-1.0,oz)),("MARK_STORE_COUNTER_CUSTOMER",(ox+2.75,oy-2.3,oz)),("MARK_STORE_COUNTER_CLERK",(ox+4.1,oy-1.5,oz)),("MARK_AISLE_END",(ox,oy-1.5,oz)),("MARK_STORE_WINDOW",(ox-3.4,front+.8,oz))]:semantic(name,loc,"staging_mark",radius_m=.55)
    semantic("INTERACT_STORE_DOOR",(ox,front-.2,oz+1),"interaction",interaction_type="door");semantic("INTERACT_COUNTER",(ox+3.2,oy-2.4,oz+1),"interaction",interaction_type="counter");semantic("INTERACT_REFRIGERATOR",(ox,back-.6,oz+1.2),"interaction",interaction_type="refrigerator")
    camera("CAM_STORE_COUNTER_TWO_SHOT",(ox+.8,oy-1.8,oz+1.65),(ox+3.35,oy-2.0,oz+1.25),52);camera("CAM_STORE_AISLE_LONG",(ox-4.3,oy-2.2,oz+1.65),(ox,oy+.4,oz+1.2),44);camera("CAM_STORE_FRONT_WIDE",(ox,front-2.8,oz+2.0),(ox,oy,oz+1.5),38)
    collider("COLLIDER_STORE_FLOOR",(ox,oy,oz-.22),(11,9,.3))


def build_alley(origin=(0,0,0),master=False):
    ox,oy,oz=origin
    cube("AlleyGround",(ox,oy,oz-.10),(6,15,.20),"MAT_CONCRETE",.02);cube("AlleyWallBuilding",(ox+3,oy,oz+4.2),(.28,15,8.4),"MAT_BRICK_RED",.05);cube("AlleyWallNeighbor",(ox-3,oy,oz+3.6),(.28,15,7.2),"MAT_BRICK_DARK",.05)
    # Patchy paving and puddles break the long corridor.
    for i,(x,y,w,d) in enumerate(((-1.3,-5,2.5,2.2),(1.1,-1.8,2.9,1.7),(-.6,2.1,3.1,2.4),(1.2,5.1,2.4,1.8))):cube(f"AlleyPatch_{i}",(ox+x,oy+y,oz+.025),(w,d,.035),"MAT_GRIME",.02,rot=(0,0,(i-2)*.06))
    cube("Puddle",(ox-.8,oy+.6,oz+.045),(2.1,1.2,.025),"MAT_GLASS",.04)
    kit("dumpster",(ox-1.5,oy+3.8,oz),rot=(0,0,.08));kit("utility_box",(ox+2.7,oy-1.0,oz+.2),rot=(0,0,-math.pi/2));kit("breaker_panel",(ox+2.78,oy+1.2,oz+.2),rot=(0,0,-math.pi/2),scale=(.8,.8,.8));kit("service_door",(ox+2.75,oy-5.2,oz),rot=(0,0,-math.pi/2))
    for y in (-4,0,4):kit("wall_light",(ox+2.78,oy+y,oz+2.4),rot=(0,0,-math.pi/2),scale=(.75,.75,.75));area_light("AlleyPractical",(ox+2.4,oy+y,oz+2.5),(ox,oy+y,oz+.6),160,1.8)
    for y in (-3.2,1.1,5.0):kit("drainpipe",(ox+2.72,oy+y,oz),scale=(.9,.9,1.6))
    kit("fire_escape",(ox+2.7,oy+2.0,oz+1.0),rot=(0,0,-math.pi/2),scale=(.8,.8,.8));kit("traffic_cone",(ox+.9,oy-3.4,oz),scale=(.85,.85,.85))
    # Trash bags and repair clutter.
    for i,(x,y,s) in enumerate(((-2.0,4.8,.45),(-1.3,5.1,.34),(-2.3,3.9,.28))):sphere(f"TrashBag_{i}",(ox+x,oy+y,oz+s*.72),(s,s*.75,s),"MAT_RUBBER");cyl("BagTie",(ox+x,oy+y,oz+s*1.55),.04,.16,"MAT_RUBBER",8)
    for i in range(3):kit("delivery_box",(ox+1.5+i*.45,oy+5.6-i*.12,oz),rot=(0,0,(i-1)*.15),scale=(.7,.7,.7))
    text_mesh("AlleyGraffiti","MAYBE EXIT",(ox-2.84,oy+.4,oz+2.0),.40,"MAT_EMISSIVE_CYAN",rot=(math.pi/2,0,math.pi/2));text_mesh("ServiceWarning","SERVICE DOOR\nDO NOT FEED",(ox+2.76,oy-3.7,oz+1.6),.18,"MAT_PAPER",rot=(math.pi/2,0,-math.pi/2))
    semantic("ALLEY_NORTH",(ox,oy+7.5,oz),"socket",socket_type="ALLEY",clearance_m=2);semantic("ALLEY_SOUTH",(ox,oy-7.5,oz),"socket",socket_type="ALLEY",clearance_m=2);semantic("LOT_SERVICE",(ox+3,oy-5.2,oz),"socket",socket_type="SERVICE",clearance_m=1.5)
    for name,loc in [("MARK_ALLEY_DUMPSTER",(ox-.3,oy+3.7,oz)),("MARK_ALLEY_DOOR",(ox+1.8,oy-5.2,oz)),("MARK_ALLEY_TWO_SHOT_A",(ox-.7,oy-.6,oz)),("MARK_ALLEY_TWO_SHOT_B",(ox+.7,oy-.6,oz))]:semantic(name,loc,"staging_mark",radius_m=.55)
    semantic("INTERACT_DUMPSTER",(ox-1.5,oy+3.8,oz+1),"interaction",interaction_type="dumpster");semantic("INTERACT_SERVICE_DOOR",(ox+2.75,oy-5.2,oz+1),"interaction",interaction_type="door")
    camera("CAM_ALLEY_LONG_LENS",(ox,oy-6.5,oz+1.7),(ox,oy+3.2,oz+1.1),66);camera("CAM_ALLEY_DUMPSTER_TWO_SHOT",(ox+1.9,oy+1.2,oz+1.65),(ox-.8,oy+3.7,oz+1.1),48);camera("CAM_ALLEY_SERVICE_DOOR",(ox-1.8,oy-3.0,oz+1.65),(ox+2.7,oy-5.1,oz+1.1),52)
    collider("COLLIDER_ALLEY_FLOOR",(ox,oy,oz-.23),(6,15,.3))


def add_hall(origin):
    ox,oy,oz=origin;cube("HallFloor",(ox,oy,oz-.08),(4.2,7,.16),"MAT_CARPET",.02);cube("HallLeft",(ox-2.1,oy,oz+1.8),(.18,7,3.6),"MAT_PLASTER_CREAM",.03);cube("HallRight",(ox+2.1,oy,oz+1.8),(.18,7,3.6),"MAT_PLASTER_CREAM",.03);cube("HallCeiling",(ox,oy,oz+3.55),(4.2,7,.14),"MAT_PLASTER_WARM",.02)
    for y in (-2.2,0,2.2):kit("entry_door",(ox-2.0,oy+y,oz),rot=(0,0,-math.pi/2),scale=(.8,.8,.8));cube("HallFixture",(ox,oy+y,oz+3.38),(.65,.65,.08),"MAT_EMISSIVE_WARM",.05,"RUNTIME_OBJECTS",kind="runtime_controlled");area_light("HallPractical",(ox,oy+y,oz+3.2),(ox,oy+y,oz),120,1.5)
    text_mesh("HallNotice","FLOOR 1?",(ox+2.0,oy-1.0,oz+1.7),.22,"MAT_PAPER",rot=(math.pi/2,0,-math.pi/2))
    semantic("MARK_HALL_CONVERSATION_A",(ox-.7,oy,oz),"staging_mark",radius_m=.55);semantic("MARK_HALL_CONVERSATION_B",(ox+.7,oy,oz),"staging_mark",radius_m=.55);camera("CAM_HALL_LONG",(ox,oy-3.0,oz+1.65),(ox,oy+2.2,oz+1.15),52)


def setup_preview(camera_obj,path,target=(0,0,1)):
    scene=bpy.context.scene;scene.camera=camera_obj;scene.render.image_settings.file_format="PNG";scene.render.filepath=str(path);scene.render.film_transparent=False;sun();area_light("PreviewKey",tuple(Vector(camera_obj.location)+Vector((-2,-1,5))),target,900,5);bpy.ops.render.render(write_still=True)


def custom_sidecar(module_id,category,source,asset,bounds):
    scene=bpy.context.scene
    def json_value(value):
        if hasattr(value,"to_list"):return value.to_list()
        if isinstance(value,(list,tuple)):return [json_value(v) for v in value]
        if isinstance(value,(str,int,float,bool)) or value is None:return value
        return str(value)
    def point(o):return {"id":o.get("semantic_id",o.name),"node":o.name,"position":[round(o.location.x,4),round(o.location.z,4),round(-o.location.y,4)],**{k:json_value(o[k]) for k in o.keys() if k not in {"semantic_kind","semantic_id","_RNA_UI"}}}
    groups={k:[] for k in ("sockets","staging_marks","camera_anchors","interactions","cutaway_groups","collision_groups")}
    for o in scene.objects:
        kind=o.get("semantic_kind")
        if kind=="socket":groups["sockets"].append(point(o))
        elif kind=="staging_mark":groups["staging_marks"].append(point(o))
        elif kind=="interaction":groups["interactions"].append(point(o))
        elif kind=="camera_anchor":groups["camera_anchors"].append(point(o))
        elif kind=="cutaway":groups["cutaway_groups"].append(point(o))
        elif kind=="collider":groups["collision_groups"].append(point(o))
    return {"schema_version":1,"module_id":module_id,"asset":str(asset.relative_to(ROOT)).replace('\\','/'),"source_blend":str(source.relative_to(ROOT)).replace('\\','/'),"category":category,"version":2 if module_id!="infinite_backlot_block" else 1,"quality_tier":"hero","bounds":{"min":bounds[0],"max":bounds[1]},**groups,"tags":["hero","production","connected","environment_art_pass"],"material_library":"assets/source/blender/world/kits/infinite_backlot_material_library.blend","detail_kit":"assets/source/blender/world/kits/infinite_backlot_detail_kit.blend","dressing_preset":"lived_in","provenance":{"author":"Infinite Backlot project","license":"project-owned","generator":"tools/blender/build_neighborhood_art_pass.py","baseline":"b09db1c3e1b3eb2bbbb8d58e1a089d45c670b43d"}}


def realize_instances():
    instances=[o for o in bpy.context.scene.objects if o.instance_type=="COLLECTION"]
    bpy.ops.object.select_all(action="DESELECT")
    for o in instances:o.select_set(True)
    if instances:
        bpy.context.view_layer.objects.active=instances[0];bpy.ops.object.duplicates_make_real(use_base_parent=True,use_hierarchy=True)


def dedupe_semantic_ids():
    """Keep composed hero semantics while ensuring the master contract is unique."""
    seen={}
    for obj in bpy.context.scene.objects:
        semantic_id=obj.get("semantic_id")
        if not semantic_id:continue
        count=seen.get(semantic_id,0);seen[semantic_id]=count+1
        if count:
            unique=f"{semantic_id}_MASTER_{count+1}"
            obj["semantic_id"]=unique
            if obj.name!=unique:obj.name=unique


def export_current(path):
    realize_instances();bpy.ops.object.select_all(action="SELECT");path.parent.mkdir(parents=True,exist_ok=True);bpy.ops.export_scene.gltf(filepath=str(path),export_format="GLB",use_selection=True,export_apply=True,export_yup=True,export_cameras=True,export_lights=True,export_extras=True)


def build_hero(module_id,builder,bounds,preview_cam):
    source,asset,category=HERO_PATHS[module_id];reset();load_kit();builder((0,0,0),False);scene=bpy.context.scene;scene["module_id"]=module_id;scene["quality_tier"]="hero";scene["art_pass_baseline"]="b09db1c"
    cam=preview_cam();setup_preview(cam,AFTER/f"{module_id}.png")
    source.parent.mkdir(parents=True,exist_ok=True);bpy.ops.wm.save_as_mainfile(filepath=str(source),compress=True);bpy.ops.file.make_paths_relative();bpy.ops.wm.save_as_mainfile(filepath=str(source),compress=True)
    side=custom_sidecar(module_id,category,source,asset,bounds);export_current(asset);digest=hashlib.sha256(asset.read_bytes()).hexdigest();side["asset_sha256"]=digest;side["glb_sha256"]=digest;side_path=asset.with_suffix(".module.json");side_path.write_text(json.dumps(side,indent=2),encoding="utf-8");return side


def build_master():
    reset();load_kit()
    # One spatially continuous block: street at y=0, apartment front at y=7,
    # lobby inside y=11, elevator/hall behind, alley west, store east.
    build_intersection((5,0,0),True)
    # The intersection's cheap thumbnail-only backdrop façades overlap the real
    # apartment, alley, and store at master-block coordinates. Remove only those
    # proxies before composing the actual connected hero locations.
    for obj in list(bpy.context.scene.objects):
        if obj.name.startswith("BackgroundFacade_") or obj.name.startswith("BgWindow"):
            bpy.data.objects.remove(obj,do_unlink=True)
    build_exterior((0,13,0),True);build_lobby((0,11,0),True);add_hall((0,18.5,0));build_alley((-10,14.5,0),True);build_store((17,11,0),True)
    # Entrance glass vestibule physically bridges sidewalk to lobby.
    cube("VestibuleFloor",(0,6.8,.02),(4.0,2.2,.14),"MAT_TILE_GREEN",.03);cube("VestibuleGlassL",(-1.9,6.8,1.7),(.08,2.2,3.4),"MAT_GLASS",.02);cube("VestibuleGlassR",(1.9,6.8,1.7),(.08,2.2,3.4),"MAT_GLASS",.02)
    # Service connector from hall/building to alley.
    cube("ServiceConnectorFloor",(-4.5,18.5,-.02),(5.8,2.8,.15),"MAT_CONCRETE",.02);cube("ServiceConnectorWall",(-4.5,19.85,1.7),(5.8,.18,3.4),"MAT_BRICK_DARK",.03);kit("service_door",(-7.15,18.5,0),rot=(0,0,-math.pi/2))
    # Distant skyline with varied setbacks; intentionally low-cost.
    for i,(x,y,w,d,h,matn) in enumerate(((-24,24,10,8,16,"MAT_BRICK_DARK"),(-13,29,8,7,12,"MAT_PAINT_TEAL"),(9,29,12,8,18,"MAT_PLASTER_WARM"),(28,27,9,7,14,"MAT_BRICK_RED"),(36,20,8,9,20,"MAT_METAL_DARK"))):
        cube(f"Skyline_{i}",(x,y,h/2-1),(w,d,h),matn,.12)
        cube(f"SkylineRoof_{i}",(x+(i%2)*.8,y,h-.6),(w*.65,d*.6,1.4),"MAT_GRIME",.08)
    # Master-only transitions and camera corridors.
    for name,loc in [("MARK_MASTER_STREET",(5,4.8,0)),("MARK_MASTER_ENTRANCE",(0,5.5,0)),("MARK_MASTER_LOBBY",(0,11,0)),("MARK_MASTER_ELEVATOR",(0,14,0)),("MARK_MASTER_HALL",(0,18.5,0)),("MARK_MASTER_ALLEY",(-10,14.5,0)),("MARK_MASTER_STORE",(17,7,0))]:semantic(name,loc,"staging_mark",radius_m=.65)
    semantic("TRANSITION_STREET_TO_ENTRANCE",(0,5.6,0),"interaction",interaction_type="transition");semantic("TRANSITION_ENTRANCE_TO_LOBBY",(0,7.5,0),"interaction",interaction_type="transition");semantic("TRANSITION_LOBBY_TO_ELEVATOR",(0,14.5,0),"interaction",interaction_type="transition");semantic("TRANSITION_HALL_TO_ALLEY",(-7.15,18.5,0),"interaction",interaction_type="transition");semantic("TRANSITION_SIDEWALK_TO_STORE",(17,6.5,0),"interaction",interaction_type="transition")
    camera("CAM_MASTER_STREET_WIDE",(17,-14,6),(0,9,4.8),40);camera("CAM_MASTER_ENTRANCE",(-4,2,1.7),(0,7,1.8),42);camera("CAM_MASTER_LOBBY",(0,8,1.7),(0,13,1.3),40);camera("CAM_MASTER_ALLEY",(-10,7.8,1.7),(-10,16,1.1),58);camera("CAM_MASTER_STORE",(17,4.2,1.7),(17,11,1.3),42)
    collider("COLLIDER_MASTER_ACTOR_GROUND",(5,8,-.30),(44,34,.35));dedupe_semantic_ids()
    scene=bpy.context.scene;scene["module_id"]="infinite_backlot_block";scene["quality_tier"]="hero";scene["continuous_spatial_layout"]=True
    # Tour camera is authored at human scale and never renders helpers.
    cam=camera("CAM_WORLD_ART_TOUR",(19,-13,2.1),(5,7,2.5),38);scene.camera=cam
    for c in (bpy.data.collections.get("SEMANTICS"),bpy.data.collections.get("CAMERAS"),bpy.data.collections.get("COLLIDERS"),bpy.data.collections.get("CUTAWAYS")):
        if c:c.hide_render=True
    MASTER_BLEND.parent.mkdir(parents=True,exist_ok=True);bpy.ops.wm.save_as_mainfile(filepath=str(MASTER_BLEND),compress=True);bpy.ops.file.make_paths_relative();bpy.ops.wm.save_as_mainfile(filepath=str(MASTER_BLEND),compress=True)
    side=custom_sidecar("infinite_backlot_block","connected_neighborhood",MASTER_BLEND,MASTER_GLB,([-30,-.5,-36],[41,21,15]));export_current(MASTER_GLB);digest=hashlib.sha256(MASTER_GLB.read_bytes()).hexdigest();side["asset_sha256"]=digest;side["glb_sha256"]=digest;MASTER_SCENE.write_text(json.dumps(side,indent=2),encoding="utf-8");return side


def update_registry(hero_sidecars,master):
    reg=json.loads(REGISTRY.read_text());reg["modules"]=[m for m in reg["modules"] if m["module_id"]!="infinite_backlot_block"];by={m["module_id"]:m for m in reg["modules"]}
    for side in hero_sidecars:
        old=by[side["module_id"]];old.update(side);old["preview"]=f"assets/reference/world-art-pass/after/{side['module_id']}.png"
    # Quality-tier classification prevents blockouts from masquerading as final art.
    background={"neighborhood_skyline_facades_a","neighborhood_storefront_row_a","neighborhood_street_straight_a","apartment_hall_straight_a","apartment_hall_short_a"}
    needs={"apartment_interior_recurring_a","neighborhood_diner_a","apartment_main_entrance_a","apartment_elevator_lobby_a"}
    hero_ids={s["module_id"] for s in hero_sidecars}
    for m in reg["modules"]:
        if m["module_id"] in hero_ids:m["quality_tier"]="hero"
        elif m["module_id"] in background:m["quality_tier"]="background"
        elif m["module_id"] in needs:m["quality_tier"]="blockout"
        else:m["quality_tier"]="blockout"
    master["preview"]="assets/reference/world-art-pass/master_neighborhood.png";reg["modules"].append(master);reg["module_count"]=len(reg["modules"]);reg["registry_version"]=2;reg["quality_tiers"]=["blockout","background","production","hero"];reg["art_direction"]="stylized_surreal_bureaucratic_neighborhood_v2";REGISTRY.write_text(json.dumps(reg,indent=2),encoding="utf-8")

hero=[]
hero.append(build_hero("apartment_exterior_a",build_exterior,([-8.2,-.5,-6.2],[7.2,15,6.2]),lambda:camera("CAM_HERO_PREVIEW",(13,-24,8.5),(0,0,5.7),36)))
hero.append(build_hero("apartment_lobby_a",build_lobby,([-4,0,-4],[4,4.3,4]),lambda:camera("CAM_HERO_PREVIEW",(-3.1,-3.35,1.8),(0,1.0,1.3),30)))
hero.append(build_hero("neighborhood_intersection_a",build_intersection,([-17.5,-.5,-17.5],[20,9,12.5]),lambda:camera("CAM_HERO_PREVIEW",(16,-16,6),(-2,4,1.2),46)))
hero.append(build_hero("neighborhood_convenience_store_a",build_store,([-5.5,0,-4.5],[5.5,4.6,4.5]),lambda:camera("CAM_HERO_PREVIEW",(10,-16,5.5),(0,-.2,2.1),38)))
hero.append(build_hero("neighborhood_alley_a",build_alley,([-3,0,-7.5],[3,8.5,7.5]),lambda:camera("CAM_HERO_PREVIEW",(-2.2,-5.8,1.8),(.2,2.2,1.3),44)))
master=build_master();update_registry(hero,master)
print(f"NEIGHBORHOOD_ART_PASS_BUILT heroes={len(hero)} master={MASTER_BLEND} glb={MASTER_GLB} registry_version=2")
