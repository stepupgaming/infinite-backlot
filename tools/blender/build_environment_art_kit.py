"""Build the reusable Infinite Backlot environment-art detail/material kits.

This script owns only repeatable detail assets. Hero-location composition is authored
separately in build_neighborhood_art_pass.py.
"""
from __future__ import annotations

import json
import math
from pathlib import Path

import bpy

ROOT=Path(r"C:/Projects/bevy-infinite")
SOURCE=ROOT/"assets/source/blender/world/kits"
RUNTIME=ROOT/"assets/world/kits"
MAT_BLEND=SOURCE/"infinite_backlot_material_library.blend"
KIT_BLEND=SOURCE/"infinite_backlot_detail_kit.blend"
KIT_GLB=RUNTIME/"infinite_backlot_detail_kit.glb"
CATALOG=RUNTIME/"infinite_backlot_detail_kit.catalog.json"
SOURCE.mkdir(parents=True,exist_ok=True); RUNTIME.mkdir(parents=True,exist_ok=True)

PALETTE={
"plaster_warm":((0.47,0.34,0.24,1),0.82,0),"plaster_cream":((0.72,0.58,0.39,1),0.78,0),
"brick_red":((0.38,0.055,0.025,1),0.88,0),"brick_dark":((0.20,0.025,0.018,1),0.92,0),
"concrete":((0.30,0.31,0.29,1),0.92,0),"sidewalk":((0.43,0.44,0.39,1),0.86,0),
"asphalt":((0.028,0.035,0.045,1),0.96,0),"paint_teal":((0.025,0.18,0.19,1),0.52,0.05),
"paint_cream":((0.82,0.67,0.42,1),0.65,0),"paint_burgundy":((0.31,0.025,0.055,1),0.58,0),
"metal_dark":((0.07,0.08,0.09,1),0.36,0.72),"metal_galvanized":((0.36,0.40,0.42,1),0.31,0.62),
"brass":((0.58,0.32,0.07,1),0.25,0.76),"wood":((0.24,0.075,0.025,1),0.68,0),
"plastic_red":((0.62,0.025,0.018,1),0.42,0),"plastic_blue":((0.025,0.16,0.42,1),0.42,0),
"rubber":((0.018,0.022,0.025,1),0.94,0),"tile_green":((0.05,0.28,0.21,1),0.34,0),
"carpet":((0.15,0.16,0.13,1),0.98,0),"glass":((0.025,0.16,0.23,0.42),0.12,0.08),
"emissive_warm":((1.0,0.36,0.06,1),0.25,0),"emissive_cyan":((0.04,0.72,0.82,1),0.22,0),
"paper":((0.82,0.75,0.57,1),0.90,0),"grime":((0.055,0.035,0.025,1),0.96,0),
"green":((0.03,0.26,0.09,1),0.82,0),"cardboard":((0.48,0.27,0.10,1),0.90,0),
}


def reset():
    bpy.ops.object.select_all(action="SELECT"); bpy.ops.object.delete(use_global=False)
    for datablocks in (bpy.data.meshes,bpy.data.curves,bpy.data.materials,bpy.data.cameras,bpy.data.lights):
        for block in list(datablocks):
            datablocks.remove(block)
    for collection in list(bpy.data.collections):
        bpy.data.collections.remove(collection)


def material(key):
    name="MAT_"+key.upper()
    found=bpy.data.materials.get(name)
    if found:return found
    color,rough,metal=PALETTE[key]
    m=bpy.data.materials.new(name); m.diffuse_color=color; m.use_nodes=True
    p=m.node_tree.nodes.get("Principled BSDF"); p.inputs["Base Color"].default_value=color; p.inputs["Roughness"].default_value=rough; p.inputs["Metallic"].default_value=metal
    if key=="glass":
        p.inputs["Alpha"].default_value=color[3]; p.inputs["Transmission Weight"].default_value=.18; m.surface_render_method="DITHERED"
    if key.startswith("emissive"):
        p.inputs["Emission Color"].default_value=color; p.inputs["Emission Strength"].default_value=4.0
    m["gltf_compatible"]=True; m["surface_family"]=key; m["shared_library"]="infinite_backlot_v2"
    return m


def move_to(obj,collection):
    for c in list(obj.users_collection):c.objects.unlink(obj)
    collection.objects.link(obj)


def cube(collection,name,loc,dims,mat_key,bevel=.04,rotation=(0,0,0)):
    bpy.ops.mesh.primitive_cube_add(location=loc,rotation=rotation); o=bpy.context.object; o.name=name; o.dimensions=dims
    bpy.ops.object.transform_apply(location=False,rotation=False,scale=True); o.data.materials.append(material(mat_key)); move_to(o,collection)
    if bevel:
        mod=o.modifiers.new("Art bevel","BEVEL");mod.width=min(bevel,min(dims)*.22);mod.segments=2
    o["asset_part"]=True; return o


def cyl(collection,name,loc,radius,depth,mat_key,vertices=16,rotation=(0,0,0)):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=radius,depth=depth,location=loc,rotation=rotation);o=bpy.context.object;o.name=name;o.data.materials.append(material(mat_key));move_to(o,collection);o["asset_part"]=True;return o


def torus(collection,name,loc,major,minor,mat_key,rotation=(0,0,0)):
    bpy.ops.mesh.primitive_torus_add(major_radius=major,minor_radius=minor,major_segments=16,minor_segments=6,location=loc,rotation=rotation);o=bpy.context.object;o.name=name;o.data.materials.append(material(mat_key));move_to(o,collection);return o


def new_asset(asset_id,category):
    c=bpy.data.collections.new("KIT_"+asset_id.upper());bpy.context.scene.collection.children.link(c);c["asset_id"]=asset_id;c["category"]=category;c["quality_tier"]="production"
    try:c.asset_mark()
    except Exception:pass
    return c


def build_window(c,double=False):
    w=2.5 if double else 1.3; cube(c,"FrameTop",(0,0,w*.0+1.25),(w+.18,.16,.14),"paint_cream");cube(c,"FrameBottom",(0,0,.08),(w+.18,.18,.14),"paint_cream")
    for x in (-w/2,w/2):cube(c,"FrameSide",(x,0,.66),(.14,.16,1.3),"paint_cream")
    if double:cube(c,"Mullion",(0,-.01,.66),(.10,.15,1.3),"paint_cream")
    cube(c,"RecessShadow",(0,.10,.66),(w,.10,1.18),"metal_dark",.01);cube(c,"Glass",(0,-.01,.66),(w-.12,.035,1.08),"glass",.01)
    cube(c,"Sill",(0,-.10,.01),(w+.35,.38,.10),"concrete")

def build_door(c,service=False):
    cube(c,"FrameTop",(0,0,2.2),(1.35,.25,.16),"paint_cream");
    for x in (-.62,.62):cube(c,"FrameSide",(x,0,1.1),(.16,.25,2.2),"paint_cream")
    cube(c,"Door",(0,.03,1.05),(1.08,.14,2.08),"metal_dark" if service else "wood")
    if not service:cube(c,"GlassInset",(0,-.05,1.5),(.64,.035,.75),"glass",.01)
    cyl(c,"Handle",(.38,-.12,1.05),.055,.18,"brass",12,rotation=(math.pi/2,0,0))

def build_canopy(c):
    cube(c,"Canopy",(0,0,2.45),(3.4,1.5,.18),"paint_teal",.10);cube(c,"CanopyTrim",(0,-.71,2.38),(3.55,.10,.30),"brass",.03)
    for x in (-1.35,1.35):cube(c,"Bracket",(x,.28,1.55),(.10,.10,1.75),"metal_dark")

def build_awning(c):
    cube(c,"Awning",(0,0,2.2),(3.6,1.25,.16),"paint_burgundy",.07,rotation=(math.radians(-8),0,0))
    for x in (-1.4,-.7,0,.7,1.4):cube(c,"Stripe",(x,-.60,2.08),(.25,.10,.28),"paint_cream",.01)

def build_fire_escape(c):
    for z in (.2,2.2,4.2):
        cube(c,"Landing",(0,0,z),(2.7,1.0,.10),"metal_dark",.01)
        for x in (-1.25,1.25):cube(c,"RailPost",(x,-.42,z+.55),(.07,.07,1.1),"metal_dark",.01)
        cube(c,"Rail",(0,-.42,z+1.05),(2.6,.06,.07),"metal_dark",.01)
    for i in range(9):cube(c,"Stair",(-1.0+i*.24,.35,2.05+i*.22),(.55,.14,.08),"metal_dark",.01)

def build_ac(c):
    cube(c,"Case",(0,0,.55),(1.35,.55,1.0),"metal_galvanized",.09);cyl(c,"Fan",(0,-.31,.58),.36,.055,"rubber",16,rotation=(math.pi/2,0,0));
    for a in range(0,360,45):cube(c,"FanBlade",(0,-.36,.58),(.07,.03,.55),"metal_dark",.01,rotation=(0,math.radians(a),0))

def build_pipe(c):
    cyl(c,"DrainPipe",(0,0,1.5),.12,3.0,"metal_galvanized",12);torus(c,"Joint",(0,0,.35),.14,.04,"metal_dark",rotation=(math.pi/2,0,0));cube(c,"Bracket",(0,.12,1.8),(.36,.08,.08),"metal_dark")

def build_utility(c):
    cube(c,"Box",(0,0,.65),(.9,.35,1.25),"metal_galvanized",.08);cube(c,"DoorInset",(0,-.19,.67),(.68,.025,.92),"paint_teal",.02);cyl(c,"Latch",(.24,-.24,.67),.035,.08,"brass",10,rotation=(math.pi/2,0,0))

def build_light(c):
    cube(c,"Backplate",(0,.10,.75),(.34,.12,.58),"metal_dark",.05);cube(c,"Glow",(0,-.02,.78),(.24,.18,.36),"emissive_warm",.08);cube(c,"Hood",(0,-.03,1.03),(.46,.28,.12),"paint_teal",.04)

def build_railing(c):
    for x in (-1.5,-.75,0,.75,1.5):cube(c,"Post",(x,0,.55),(.07,.07,1.1),"metal_dark",.01)
    cube(c,"TopRail",(0,0,1.08),(3.1,.09,.09),"metal_dark",.03);cube(c,"MidRail",(0,0,.55),(3.1,.06,.06),"metal_dark",.02)

def build_street_lamp(c):
    cyl(c,"Pole",(0,0,2.0),.10,4.0,"metal_dark",14);cyl(c,"Base",(0,0,.18),.28,.36,"metal_dark",14);cube(c,"Arm",(.35,0,3.85),(.75,.08,.08),"metal_dark",.02);cube(c,"Lamp",(.72,0,3.72),(.42,.34,.18),"emissive_warm",.08)

def build_hydrant(c):
    cyl(c,"Body",(0,0,.42),.22,.74,"plastic_red",12);cyl(c,"Cap",(0,0,.86),.28,.16,"plastic_red",12);cyl(c,"Side",(.28,0,.48),.12,.28,"brass",12,rotation=(0,math.pi/2,0));torus(c,"Flange",(0,0,.12),.25,.05,"metal_dark")

def build_dumpster(c):
    cube(c,"Body",(0,0,.75),(2.2,1.1,1.35),"paint_teal",.12);cube(c,"Lid",(0,.06,1.48),(2.25,1.08,.12),"metal_dark",.06,rotation=(math.radians(4),0,0));
    for x in (-.85,.85):cyl(c,"Wheel",(x,-.45,.12),.12,.10,"rubber",12,rotation=(math.pi/2,0,0))

def build_bench(c):
    cube(c,"Seat",(0,0,.48),(2.4,.56,.12),"wood",.06);cube(c,"Back",(0,.25,1.02),(2.4,.12,.82),"wood",.06,rotation=(math.radians(-6),0,0));
    for x in (-.85,.85):cube(c,"Leg",(x,0,.24),(.10,.48,.48),"metal_dark",.02)

def build_planter(c):
    cyl(c,"Pot",(0,0,.35),.42,.70,"paint_burgundy",12);cyl(c,"Trunk",(0,0,1.05),.10,1.0,"wood",10)
    for loc,r in [((0,0,1.75),.62),((-.35,0,1.45),.38),((.35,.05,1.5),.42)]:
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1,radius=r,location=loc);o=bpy.context.object;o.name="Leaves";o.data.materials.append(material("green"));move_to(o,c)

def build_drain(c):
    cube(c,"DrainFrame",(0,0,.03),(1.0,.55,.06),"metal_dark",.02)
    for x in (-.38,-.19,0,.19,.38):cube(c,"Slot",(x,0,.075),(.07,.44,.035),"metal_galvanized",.01)

def build_cone(c):
    bpy.ops.mesh.primitive_cone_add(vertices=16,radius1=.28,radius2=.06,depth=.72,location=(0,0,.38));o=bpy.context.object;o.name="Cone";o.data.materials.append(material("plastic_red"));move_to(o,c);cube(c,"Base",(0,0,.05),(.65,.65,.10),"rubber",.04);torus(c,"Stripe",(0,0,.48),.15,.045,"paper")

def build_bike_rack(c):
    for x in (-.8,0,.8):
        torus(c,"RackLoop",(x,0,.6),.53,.055,"metal_galvanized",rotation=(math.pi/2,0,0));cube(c,"Foot",(x,0,.06),(.18,.55,.10),"metal_dark",.02)

def build_mailboxes(c):
    cube(c,"Bank",(0,.08,1.15),(3.2,.35,2.1),"metal_galvanized",.06)
    for row in range(4):
        for col in range(6):
            x=-1.3+col*.52;z=.38+row*.48;cube(c,f"Door_{row}_{col}",(x,-.12,z),(.45,.035,.39),"paint_teal" if (row+col)%5==0 else "metal_dark",.015);cyl(c,"Latch",(x+.13,-.16,z),.018,.04,"brass",8,rotation=(math.pi/2,0,0))

def build_notice(c):
    cube(c,"Frame",(0,0,1.25),(2.2,.12,1.55),"wood",.04);cube(c,"Board",(0,-.08,1.25),(1.95,.035,1.30),"paint_teal",.01)
    for i,(x,z) in enumerate([(-.55,1.55),(.35,1.48),(-.35,.98),(.55,1.02)]):cube(c,f"Flyer_{i}",(x,-.105,z),(.48,.018,.42),"paper",.005,rotation=(0,math.radians((i-2)*2),0))

def build_chair(c):
    cube(c,"Seat",(0,0,.48),(.72,.72,.18),"paint_burgundy",.12);cube(c,"Back",(0,.30,1.10),(.72,.18,1.05),"paint_burgundy",.12,rotation=(math.radians(-5),0,0));
    for x in (-.26,.26):cube(c,"Leg",(x,0,.22),(.08,.52,.44),"metal_dark",.02)

def build_shelf(c):
    for z in (.25,.85,1.45,2.05):cube(c,"Shelf",(0,0,z),(2.4,.8,.10),"metal_dark",.03)
    for x in (-1.12,1.12):cube(c,"Upright",(x,0,1.15),(.10,.8,2.3),"paint_teal",.03)
    colors=["cardboard","plastic_red","plastic_blue","paper"]
    for row,z in enumerate((.5,1.1,1.7)):
        for col in range(6):cube(c,f"Product_{row}_{col}",(-.9+col*.36,-.05,z),(.24,.32,.34),colors[(row+col)%4],.025)

def build_fridge(c):
    cube(c,"Case",(0,.18,1.25),(2.8,.65,2.5),"metal_dark",.05)
    for col in range(3):
        x=-.9+col*.9;cube(c,"GlassDoor",(x,-.18,1.28),(.78,.045,2.15),"glass",.02);cube(c,"Handle",(x+.30,-.25,1.28),(.035,.05,1.2),"metal_galvanized",.01)
        for row in range(4):
            z=.48+row*.46
            for j in range(3):cyl(c,"Bottle",(x-.22+j*.22,-.05,z),.055,.25,["plastic_red","plastic_blue","paper"][(row+j)%3],8)
    cube(c,"Header",(0,-.15,2.58),(2.85,.20,.30),"emissive_cyan",.04)

def build_counter(c):
    cube(c,"Counter",(0,0,.55),(3.2,1.0,1.05),"wood",.10);cube(c,"Top",(0,-.02,1.12),(3.4,1.08,.16),"brass",.05);cube(c,"Register",(.85,-.18,1.40),(.58,.55,.42),"metal_dark",.08);cube(c,"Screen",(.85,-.48,1.48),(.42,.03,.24),"emissive_cyan",.02)

def build_box(c):
    cube(c,"Box",(0,0,.32),(.75,.58,.64),"cardboard",.04);cube(c,"Tape",(0,-.30,.35),(.10,.02,.55),"paper",.005)

def build_breaker(c):
    cube(c,"Case",(0,0,.85),(1.2,.28,1.6),"metal_galvanized",.06);cube(c,"Door",(0,-.16,.87),(1.0,.03,1.35),"paint_teal",.02)
    for row in range(5):
        for col in (-.22,.22):cube(c,"Switch",(col,-.21,.45+row*.20),(.16,.06,.08),"rubber",.02)

BUILDERS={
"window_single":("architectural",lambda c:build_window(c,False)),"window_double":("architectural",lambda c:build_window(c,True)),
"entry_door":("architectural",lambda c:build_door(c,False)),"service_door":("architectural",lambda c:build_door(c,True)),
"canopy":("architectural",build_canopy),"awning":("architectural",build_awning),"fire_escape":("architectural",build_fire_escape),
"ac_unit":("architectural",build_ac),"drainpipe":("architectural",build_pipe),"utility_box":("architectural",build_utility),
"wall_light":("architectural",build_light),"railing":("architectural",build_railing),
"street_lamp":("street",build_street_lamp),"hydrant":("street",build_hydrant),"dumpster":("street",build_dumpster),
"bench":("street",build_bench),"planter":("street",build_planter),"storm_drain":("street",build_drain),
"traffic_cone":("street",build_cone),"bike_rack":("street",build_bike_rack),
"mailbox_bank":("interior",build_mailboxes),"notice_board":("interior",build_notice),"lobby_chair":("interior",build_chair),
"store_shelf_stocked":("interior",build_shelf),"store_fridge_stocked":("interior",build_fridge),"checkout_counter":("interior",build_counter),
"delivery_box":("interior",build_box),"breaker_panel":("interior",build_breaker),"potted_plant":("interior",build_planter),
}


def save_material_library():
    reset();scene=bpy.context.scene;scene.unit_settings.system="METRIC"
    swatches=bpy.data.collections.new("MATERIAL_SWATCHES");scene.collection.children.link(swatches)
    for i,key in enumerate(PALETTE):
        material(key);x=(i%7)*1.4;y=(i//7)*1.4;cube(swatches,"SWATCH_"+key,(x,y,.25),(1.0,1.0,.5),key,.08)
    scene["library_id"]="infinite_backlot_materials_v2";scene["material_count"]=len(PALETTE)
    bpy.ops.wm.save_as_mainfile(filepath=str(MAT_BLEND),compress=True)


def save_detail_kit():
    reset();scene=bpy.context.scene;scene.unit_settings.system="METRIC";preview=bpy.data.collections.new("KIT_PREVIEW_INSTANCES");scene.collection.children.link(preview)
    catalog=[]
    for i,(asset_id,(category,builder)) in enumerate(BUILDERS.items()):
        c=new_asset(asset_id,category);builder(c)
        inst=bpy.data.objects.new("PREVIEW_"+asset_id,None);inst.instance_type="COLLECTION";inst.instance_collection=c;preview.objects.link(inst);inst.location=((i%6)*4.4,(i//6)*4.4,0)
        for o in c.objects:o.hide_set(True)
        catalog.append({"asset_id":asset_id,"collection":c.name,"category":category,"quality_tier":"production","source":str(KIT_BLEND.relative_to(ROOT)).replace('\\','/'),"materials":sorted({m.name for o in c.objects for m in getattr(o.data,'materials',[])})})
    scene["library_id"]="infinite_backlot_detail_kit_v2";scene["asset_count"]=len(catalog);bpy.ops.wm.save_as_mainfile(filepath=str(KIT_BLEND),compress=True)
    # Export original collections at origin; preview instances are excluded.
    for o in scene.objects:o.select_set(False)
    for c in bpy.data.collections:
        if c.name.startswith("KIT_") and c.name!="KIT_PREVIEW_INSTANCES":
            for o in c.objects:o.hide_set(False);o.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(KIT_GLB),export_format="GLB",use_selection=True,export_apply=True,export_yup=True,export_cameras=False,export_lights=False,export_extras=True)
    CATALOG.write_text(json.dumps({"schema_version":1,"library_id":"infinite_backlot_detail_kit_v2","assets":catalog},indent=2),encoding="utf-8")
    print(f"DETAIL_KIT_BUILT assets={len(catalog)} materials={len(PALETTE)} blend={KIT_BLEND} glb={KIT_GLB}")

save_material_library();save_detail_kit()
