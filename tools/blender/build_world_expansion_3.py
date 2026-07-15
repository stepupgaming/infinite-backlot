"""Build the CC0-adapted v3 asset library and five dressed recurring locations."""
from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path

import bpy
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/"assets/source/blender/world"
POLY=ROOT/"assets/source/polyhaven"
KIT=SOURCE/"kits/infinite_backlot_asset_library_v3.blend"
KIT_GLB=ROOT/"assets/world/kits/infinite_backlot_asset_library_v3.glb"
CATALOG=ROOT/"assets/world/kits/infinite_backlot_asset_library_v3.catalog.json"
LOCATION_SOURCE=SOURCE/"locations"
LOCATION_RUNTIME=ROOT/"assets/world/locations"
REFERENCE=ROOT/"assets/reference/world-expansion-3"
REGISTRY=ROOT/"assets/world/registry.json"

PALETTE={
"IB3_BURGUNDY":((.24,.035,.055,1),.05,.72),"IB3_RUST":((.32,.075,.028,1),.18,.78),"IB3_TEAL":((.02,.24,.25,1),.12,.55),"IB3_BRASS":((.42,.24,.055,1),.65,.35),
"IB3_CYAN":((.02,.55,.72,1),.05,.32),"IB3_WARM":((.95,.48,.16,1),.05,.5),"IB3_CONCRETE":((.27,.25,.23,1),0,.88),"IB3_ASPHALT":((.055,.06,.075,1),0,.96),
"IB3_GLASS":((.08,.22,.26,1),.08,.18),"IB3_METAL":((.12,.14,.16,1),.72,.42),"IB3_RUBBER":((.025,.027,.03,1),0,.98),"IB3_WOOD":((.28,.13,.055,1),0,.68),
"IB3_PAPER":((.72,.64,.47,1),0,.9),"IB3_TILE":((.16,.31,.32,1),0,.5),"IB3_WHITE":((.72,.72,.66,1),0,.72),"IB3_YELLOW":((.82,.52,.05,1),.03,.58),
"IB3_GREEN":((.075,.24,.12,1),0,.86),"IB3_SOIL":((.09,.055,.03,1),0,.98),"IB3_RED":((.55,.03,.035,1),.08,.58),"IB3_BLUE":((.035,.12,.42,1),.06,.58),
"IB3_PINK":((.55,.06,.28,1),0,.58),"IB3_CREAM":((.72,.62,.46,1),0,.76),"IB3_DARK_TEAL":((.01,.09,.10,1),.18,.62),"IB3_SILVER":((.38,.42,.45,1),.8,.28),
}
TEXTURE_MATERIALS={
"IB3_PH_ASPHALT":POLY/"asphalt_02/asphalt_02_diffuse_1k.jpg","IB3_PH_CHIPPED_CONCRETE":POLY/"chipped_concrete/chipped_concrete_diffuse_1k.exr",
"IB3_PH_METAL_GRATE":POLY/"metal_grate_rusty/metal_grate_rusty_diffuse_1k.jpg","IB3_PH_BRICK_FLOOR":POLY/"brick_floor_003/brick_floor_003_diffuse_1k.jpg","IB3_PH_GREEN_RUST":POLY/"green_metal_rust/green_metal_rust_diffuse_1k.jpg",
}
HDRI_PRESETS=[("IB3_HDRI_URBAN_ALLEY",POLY/"urban_alley_01/urban_alley_01_1k.hdr"), ("IB3_HDRI_INDUSTRIAL_SUNSET",POLY/"industrial_sunset/industrial_sunset_1k.hdr"), ("IB3_HDRI_ABANDONED_BAKERY",POLY/"abandoned_bakery/abandoned_bakery_1k.hdr")]
IMPORTED={
"PH_CASH_REGISTER_ADAPTED":("CashRegister_01",.65,"interior","IB3_DARK_TEAL",["odd_hours","diner"]),
"PH_PLASTIC_CRATE_ADAPTED":("plastic_crate_01",.62,"containers","IB3_TEAL",["store","workshop","alley"]),
"PH_TOOL_CHEST_ADAPTED":("metal_tool_chest",1.45,"workshop","IB3_BURGUNDY",["workshop","basement"]),
"PH_TRANSIT_BENCH_ADAPTED":("painted_wooden_bench",2.2,"street","IB3_TEAL",["transit","park"]),
"PH_STREET_LAMP_ADAPTED":("street_lamp_01",4.8,"street","IB3_METAL",["street","transit"]),
}
PROCEDURAL=[
("ARCH_CORNER_WINDOW","architecture"),("ARCH_SHOP_ENTRANCE","architecture"),("ARCH_EXTERIOR_STAIRS","architecture"),("ARCH_FIRE_ESCAPE","architecture"),("ARCH_ROOF_HVAC","architecture"),("ARCH_VENT_UNIT","architecture"),("ARCH_SERVICE_DOOR","architecture"),("ARCH_ROLLING_SHUTTER","architecture"),("ARCH_BALCONY","architecture"),("ARCH_RAILING","architecture"),("ARCH_CONDUIT_BUNDLE","architecture"),("ARCH_DRAINPIPE","architecture"),("ARCH_CANOPY","architecture"),("ARCH_LOADING_DOCK","architecture"),
("STREET_TRANSIT_MAP","street"),("STREET_NEWSPAPER_BOX","street"),("STREET_PARKING_METER","street"),("STREET_UTILITY_CABINET","street"),("STREET_TRAFFIC_SIGNAL","street"),("STREET_SIGNPOST","street"),("STREET_TRASH_RECYCLING","street"),("STREET_BOLLARD_SET","street"),("STREET_PLANTER","street"),("STREET_TREE_GUARD","street"),("STREET_BIKE_RACK","street"),("STREET_DELIVERY_CART","street"),("STREET_FOLDING_BARRIER","street"),("STREET_WORK_ZONE","street"),
("INT_WASHER","interior"),("INT_DRYER_BANK","interior"),("INT_LAUNDRY_CART","interior"),("INT_FOLDING_TABLE","interior"),("INT_DINER_BOOTH","interior"),("INT_CAFE_TABLE","interior"),("INT_COUNTER_STOOL","interior"),("INT_STORE_SHELF","interior"),("INT_VENDING_MACHINE","interior"),("INT_MAGAZINE_RACK","interior"),("INT_MAILBOX_BANK","interior"),("INT_BUILDING_DIRECTORY","interior"),("INT_WORKBENCH","workshop"),("INT_TOOL_WALL","workshop"),("INT_LOCKER_BANK","workshop"),
]


def factory():
 bpy.ops.wm.read_factory_settings(use_empty=True); s=bpy.context.scene;s.world=bpy.data.worlds.new("IB3_WORLD"); s.render.engine="BLENDER_EEVEE"; s.render.resolution_x=640;s.render.resolution_y=360;s.render.resolution_percentage=100;s.render.image_settings.file_format="PNG";s.render.film_transparent=False

def material(name,color=(.2,.2,.2,1),metallic=0,roughness=.7):
 m=bpy.data.materials.get(name) or bpy.data.materials.new(name);m.use_nodes=True;b=m.node_tree.nodes.get("Principled BSDF");b.inputs["Base Color"].default_value=color;(b.inputs.get("Metallic IOR Level") or b.inputs.get("Metallic")).default_value=metallic;b.inputs["Roughness"].default_value=roughness
 if name=="IB3_CYAN": b.inputs["Emission Color"].default_value=color;b.inputs["Emission Strength"].default_value=3.5
 return m

def all_materials():
 for n,(c,met,rough) in PALETTE.items():material(n,c,met,rough)
 for n,path in TEXTURE_MATERIALS.items():
  m=material(n,(.25,.25,.25,1),0,.82);nt=m.node_tree;bs=nt.nodes.get("Principled BSDF");img=nt.nodes.get("IB3_SOURCE") or nt.nodes.new("ShaderNodeTexImage");img.name="IB3_SOURCE";img.image=bpy.data.images.load(str(path),check_existing=True);nt.links.new(img.outputs["Color"],bs.inputs["Base Color"])

def move(obj,col):
 for old in list(obj.users_collection):old.objects.unlink(obj)
 col.objects.link(obj)

def box(col,name,loc,scale,mat="IB3_CONCRETE",bevel=.04):
 bpy.ops.mesh.primitive_cube_add(size=1,location=loc);o=bpy.context.object;o.name=name;o.dimensions=scale;bpy.ops.object.transform_apply(location=False,rotation=False,scale=True);move(o,col);o.data.materials.append(bpy.data.materials[mat])
 if bevel:
  mod=o.modifiers.new("IB3_BEVEL","BEVEL");mod.width=bevel;mod.segments=2
 return o

def cyl(col,name,loc,r,depth,mat="IB3_METAL",vertices=16):
 bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=r,depth=depth,location=loc);o=bpy.context.object;o.name=name;move(o,col);o.data.materials.append(bpy.data.materials[mat]);return o

def mark(col,asset_id,category,source="project_owned"):
 if hasattr(col,"asset_mark"):col.asset_mark();col.asset_data.author="Infinite Backlot";col.asset_data.description=f"{asset_id} / {category} / {source}"

def procedural_asset(asset_id,category):
 c=bpy.data.collections.new(asset_id);base="IB3_TEAL" if category=="street" else "IB3_BURGUNDY" if category=="architecture" else "IB3_CREAM"
 seed=sum(map(ord,asset_id));w=.7+(seed%7)*.12;d=.45+((seed//7)%5)*.1;h=.7+((seed//11)%8)*.18
 box(c,asset_id+"_BODY",(0,0,h/2),(w,d,h),base,.06)
 # Add authored silhouette/detail rather than a lone primitive.
 if "RAILING" in asset_id or "BIKE_RACK" in asset_id or "TREE_GUARD" in asset_id:
  for i in range(4):cyl(c,asset_id+f"_BAR_{i}",(-w*.4+i*w*.27,0,h*.7),.035,h*1.2,"IB3_METAL",10)
 elif "WASHER" in asset_id or "DRYER" in asset_id or "VENDING" in asset_id:
  for i in range(2):cyl(c,asset_id+f"_PORT_{i}",(-w*.22+i*w*.44,-d*.51,h*.62),w*.17,.025,"IB3_GLASS",20)
  box(c,asset_id+"_PANEL",(0,-d*.54,h*.88),(w*.72,.035,h*.12),"IB3_CYAN",.015)
 elif "SIGN" in asset_id or "MAP" in asset_id or "DIRECTORY" in asset_id or "NEWSPAPER" in asset_id:
  box(c,asset_id+"_FACE",(0,-d*.53,h*.68),(w*.82,.035,h*.55),"IB3_PAPER",.02);box(c,asset_id+"_STRIPE",(0,-d*.56,h*.78),(w*.6,.02,h*.07),"IB3_CYAN",.01)
 elif "STAIR" in asset_id or "FIRE_ESCAPE" in asset_id:
  for i in range(5):box(c,asset_id+f"_STEP_{i}",(-w*.35+i*w*.17,0,.1+i*.16),(w*.28,d,.12),"IB3_METAL",.02)
 elif "CONDUIT" in asset_id or "DRAINPIPE" in asset_id:
  for i in range(3):cyl(c,asset_id+f"_PIPE_{i}",(-.16+i*.16,0,h*.65),.045,h*1.25,"IB3_TEAL",12)
 elif "TABLE" in asset_id or "WORKBENCH" in asset_id:
  box(c,asset_id+"_TOP",(0,0,h),(w*1.2,d*1.3,.12),"IB3_WOOD",.04)
  for x in (-w*.45,w*.45):
   for y in (-d*.45,d*.45):cyl(c,asset_id+f"_LEG_{x}_{y}",(x,y,h*.5),.045,h,"IB3_METAL",10)
 else:
  box(c,asset_id+"_TRIM",(0,-d*.53,h*.72),(w*.8,.04,h*.12),"IB3_BRASS",.02);cyl(c,asset_id+"_UTILITY",(w*.32,0,h*.28),.08,d*1.05,"IB3_RUST",12)
 mark(c,asset_id,category);return c

def import_asset(asset_id,source_id,target,category,mat_name):
 c=bpy.data.collections.new(asset_id);before=set(bpy.data.objects);gltf=next((POLY/source_id).glob("*.gltf"));bpy.ops.import_scene.gltf(filepath=str(gltf));objects=[o for o in bpy.data.objects if o not in before]
 for o in objects:move(o,c)
 meshes=[o for o in objects if o.type=="MESH"]
 for o in meshes:
  o.data.materials.clear();o.data.materials.append(bpy.data.materials[mat_name])
  if len(o.data.polygons)>18000:
   mod=o.modifiers.new("IB3_DECIMATE","DECIMATE");mod.ratio=max(.2,18000/len(o.data.polygons))
 corners=[]
 for o in meshes:
  corners.extend([o.matrix_world@Vector(corner) for corner in o.bound_box])
 mn=Vector((min(v.x for v in corners),min(v.y for v in corners),min(v.z for v in corners)));mx=Vector((max(v.x for v in corners),max(v.y for v in corners),max(v.z for v in corners)));size=max(mx-mn);scale=target/max(size,1e-6);center=(mn+mx)/2
 root=bpy.data.objects.new(asset_id+"_ADAPT_ROOT",None);c.objects.link(root);root.scale=(scale,)*3;root.location=(-center.x*scale,-center.y*scale,-mn.z*scale)
 for o in objects:
  if o.parent not in objects:o.parent=root
 mark(c,asset_id,category,"Poly Haven CC0 adapted");return c

def instance(col,asset,loc,scale=1,rot=0):
 o=bpy.data.objects.new(asset+"_INSTANCE",None);o.instance_type="COLLECTION";o.instance_collection=bpy.data.collections[asset];o.location=loc;o.scale=(scale,)*3;o.rotation_euler[2]=rot;col.objects.link(o);return o

def look_at(camera,target):camera.rotation_euler=(Vector(target)-camera.location).to_track_quat('-Z','Y').to_euler()

def setup_camera(loc,target,lens=40):
 bpy.ops.object.camera_add(location=loc);c=bpy.context.object;c.name="CAM_HERO";c.data.lens=lens;look_at(c,target);bpy.context.scene.camera=c

def light_rig():
 bpy.context.scene.world.color=(.012,.016,.025)
 bpy.ops.object.light_add(type="AREA",location=(3,-4,7));a=bpy.context.object;a.data.energy=900;a.data.shape="DISK";a.data.size=7;look_at(a,(0,0,1))
 bpy.ops.object.light_add(type="AREA",location=(-4,2,4));b=bpy.context.object;b.data.energy=550;b.data.color=(.1,.7,1);b.data.size=5;look_at(b,(0,0,1))
 bpy.ops.object.light_add(type="POINT",location=(0,0,3));bpy.context.object.data.energy=350;bpy.context.object.data.color=(1,.35,.12)

def render(path):path.parent.mkdir(parents=True,exist_ok=True);bpy.context.scene.render.filepath=str(path);bpy.ops.render.render(write_still=True)

def build_library():
 factory();all_materials();catalog=[]
 for asset_id,category in PROCEDURAL:
  c=procedural_asset(asset_id,category);catalog.append(record(asset_id,category,"project_owned",[]))
 for asset_id,(source_id,target,category,mat_name,locations) in IMPORTED.items():
  c=import_asset(asset_id,source_id,target,category,mat_name);catalog.append(record(asset_id,category,f"Poly Haven CC0:{source_id}",locations))
 show=bpy.data.collections.new("IB3_ASSET_PREVIEW_GRID");bpy.context.scene.collection.children.link(show)
 for i,item in enumerate(catalog):instance(show,item["asset_id"],((i%8)*2.4-8.4,(i//8)*2.4-5.5,0),.8)
 bpy.context.scene["asset_count"]=len(catalog);bpy.context.scene["material_count"]=len(PALETTE)+len(TEXTURE_MATERIALS);bpy.context.scene["hdri_presets"]=json.dumps([n for n,_ in HDRI_PRESETS])
 setup_camera((0,-20,18),(0,1,0),48);light_rig();KIT.parent.mkdir(parents=True,exist_ok=True);bpy.ops.wm.save_as_mainfile(filepath=str(KIT),compress=True);render(REFERENCE/"asset_library.png")
 bpy.ops.export_scene.gltf(filepath=str(KIT_GLB),export_format="GLB",use_selection=False,export_cameras=False,export_lights=False,export_extras=True,export_apply=True)
 data={"schema_version":1,"library_id":"infinite_backlot_asset_library_v3","asset_count":len(catalog),"material_count":len(PALETTE)+len(TEXTURE_MATERIALS),"hdri_presets":[{"id":n,"source":p.relative_to(ROOT).as_posix(),"license":"CC0"} for n,p in HDRI_PRESETS],"runtime_asset":KIT_GLB.relative_to(ROOT).as_posix(),"assets":catalog};CATALOG.write_text(json.dumps(data,indent=2)+"\n");return catalog

def record(asset_id,category,source,locations):
 return {"asset_id":asset_id,"category":category,"search_tags":[category,"infinite_backlot","adapted"],"source":source,"license":"CC0" if source.startswith("Poly Haven") else "project_owned","material_dependencies":["IB3 shared materials"],"runtime_path":KIT_GLB.relative_to(ROOT).as_posix(),"collider_status":"simple_box_recommended","navigation_relevance":"obstacle" if category in {"street","interior","workshop","containers"} else "boundary_or_decoration","recommended_location_types":locations or [category],"thumbnail":REFERENCE.relative_to(ROOT).as_posix()+"/asset_library.png"}

def append_assets(names):
 with bpy.data.libraries.load(str(KIT),link=False) as (src,dst):dst.collections=[n for n in names if n in src.collections]

def location_materials():all_materials()

def shell(col,name,size,floor_mat,wall_mat):
 w,d,h=size;box(col,name+"_FLOOR",(0,0,-.12),(w,d,.24),floor_mat,.02);box(col,name+"_BACK",(0,d/2,h/2),(w,.18,h),wall_mat,.03);box(col,name+"_LEFT",(-w/2,0,h/2),(.18,d,h),wall_mat,.03);box(col,name+"_RIGHT",(w/2,0,h/2),(.18,d,h),wall_mat,.03)

def semantic_helpers(module,col,size,portals,interactions,colliders,staging):
 w,d,_=size;nav=box(col,"NAV_REGION_"+module.upper(),(0,0,.03),(w-.7,d-.7,.04),"IB3_CYAN",0);nav.hide_render=True;nav.hide_set(True);nav["semantic_type"]="walkable_region"
 for p in portals:
  runtime=p["position"];loc=(runtime[0],-runtime[2],1.0)
  o=box(col,p["id"],loc,(p["width"],.1,2),"IB3_CYAN",0);o.hide_render=True;o.hide_set(True);o["semantic_type"]="nav_portal"
 for item in interactions:
  runtime=item["position"];loc=(runtime[0],-runtime[2],runtime[1])
  o=box(col,item["id"],loc,(.6,.6,1.8),"IB3_WARM",0);o.hide_render=True;o.hide_set(True);o["semantic_type"]="interaction_volume"
 for item in staging:
  runtime=item["position"];loc=(runtime[0],-runtime[2],.04)
  o=box(col,item["id"],loc,(.45,.45,.04),"IB3_YELLOW",0);o.hide_render=True;o.hide_set(True);o["semantic_type"]="staging_mark"

def build_location(spec):
 factory();location_materials();append_assets(spec["assets"]);col=bpy.data.collections.new(spec["id"]);bpy.context.scene.collection.children.link(col);shell(col,spec["id"],spec["size"],spec["floor_mat"],spec["wall_mat"])
 for asset,loc,scale,rot in spec["placements"]:instance(col,asset,loc,scale,rot)
 semantic_helpers(spec["id"],col,spec["size"],spec["portals"],spec["interactions"],spec["colliders"],spec["staging"]);setup_camera(spec["camera"],spec["target"],spec.get("lens",42));light_rig();bpy.context.scene["module_id"]=spec["id"];bpy.context.scene["quality_tier"]="hero"
 source=LOCATION_SOURCE/(spec["id"]+".blend");runtime=LOCATION_RUNTIME/(spec["id"]+".glb");preview=REFERENCE/"hero_previews"/(spec["id"]+".png");source.parent.mkdir(parents=True,exist_ok=True);runtime.parent.mkdir(parents=True,exist_ok=True);bpy.ops.wm.save_as_mainfile(filepath=str(source),compress=True);render(preview)
 # Realize collection instances only in export state; source remains editable.
 bpy.ops.object.select_all(action="DESELECT")
 for o in list(bpy.context.scene.objects):
  if o.instance_type=="COLLECTION":o.select_set(True);bpy.context.view_layer.objects.active=o
 if bpy.context.selected_objects:bpy.ops.object.duplicates_make_real(use_base_parent=True,use_hierarchy=True)
 bpy.ops.export_scene.gltf(filepath=str(runtime),export_format="GLB",use_selection=False,use_visible=True,export_cameras=True,export_lights=False,export_extras=True,export_apply=True)
 side={"schema_version":1,"module_id":spec["id"],"asset":runtime.relative_to(ROOT).as_posix(),"source_blend":source.relative_to(ROOT).as_posix(),"category":"recurring_location","version":1,"quality_tier":"hero","walkable_regions":[{"id":"NAV_REGION_"+spec["id"].upper(),"polygon":[[-spec["size"][0]/2+.35,-spec["size"][1]/2+.35],[spec["size"][0]/2-.35,-spec["size"][1]/2+.35],[spec["size"][0]/2-.35,spec["size"][1]/2-.35],[-spec["size"][0]/2+.35,spec["size"][1]/2-.35]],"height":0,"surface_type":"interior","actor_clearance":.34}],"portals":spec["portals"],"colliders":spec["colliders"],"interactions":spec["interactions"],"staging_marks":spec["staging"],"camera_anchors":[{"id":"CAM_"+spec["id"].upper(),"position":list(spec["camera"]),"look_at":list(spec["target"])}],"cutaway_groups":["CUTAWAY_FRONT"],"runtime_doors":[p["control_entity"] for p in spec["portals"] if p.get("control_entity")],"provenance":{"author":"Infinite Backlot project","license":"project-owned adaptation with Poly Haven CC0 sources","source":"Infinite Backlot v3 world expansion","polyhaven_cc0_assets":[IMPORTED[a][0] for a in spec["assets"] if a in IMPORTED],"manifest":"assets/world/kits/polyhaven_cc0_intake.provenance.json"},"preview":preview.relative_to(ROOT).as_posix(),"glb_sha256":hashlib.sha256(runtime.read_bytes()).hexdigest()};(runtime.with_suffix(".scene.json")).write_text(json.dumps(side,indent=2)+"\n");return side

def specs():
 door=lambda i,x,z,control:{"id":i,"position":[x,0.0,z],"width":1.5,"regions":["inside","outside"],"runtime_open":True,"control_entity":control}
 inter=lambda i,x,y,z,t:{"id":i,"position":[x,y,z],"interaction_type":t}
 coll=lambda i,c,h:{"id":i,"shape":"box","center":c,"half_extents":h,"role":"static"}
 result=[
 {"id":"location_odd_hours_v3","size":(10,9,3.5),"floor_mat":"IB3_PH_BRICK_FLOOR","wall_mat":"IB3_BURGUNDY","assets":["PH_CASH_REGISTER_ADAPTED","PH_PLASTIC_CRATE_ADAPTED","INT_STORE_SHELF","INT_VENDING_MACHINE","INT_MAGAZINE_RACK","ARCH_SHOP_ENTRANCE","INT_COUNTER_STOOL","STREET_NEWSPAPER_BOX"],"placements":[("PH_CASH_REGISTER_ADAPTED",(2.6,2.2,0),1,0),("PH_PLASTIC_CRATE_ADAPTED",(-3,2.6,0),1,0),("INT_STORE_SHELF",(-2,0,0),1.5,0),("INT_STORE_SHELF",(.2,0,0),1.5,0),("INT_VENDING_MACHINE",(3.6,2.8,0),1.2,0),("INT_MAGAZINE_RACK",(-3.8,-1.5,0),1,0),("INT_COUNTER_STOOL",(1.8,1.3,0),1,0),("STREET_NEWSPAPER_BOX",(3.8,-2.8,0),.8,0)],"portals":[door("NAV_PORTAL_ODD_HOURS_V3",0,-4.4,"DOOR_ODD_HOURS_V3")],"interactions":[inter("INTERACT_REGISTER",2.6,1,2.2,"counter"),inter("INTERACT_PICKUP",1.8,1,1.5,"pickup"),inter("INTERACT_STORE_DOOR",0,1,-4.2,"door")],"colliders":[coll("COLLIDER_COUNTER",[2.7,.7,2.2],[1.4,.7,.7]),coll("COLLIDER_SHELF_A",[-2,.8,0],[.8,.8,1.5]),coll("COLLIDER_SHELF_B",[.2,.8,0],[.8,.8,1.5])],"staging":[{"id":"MARK_COUNTER_CUSTOMER","position":[1.4,0,1.3]},{"id":"MARK_AISLE","position":[1.4,0,-.5]}],"camera":(9,-12,7),"target":(0,0,1.2)},
 {"id":"location_apartment_lobby_v3","size":(9,8,3.6),"floor_mat":"IB3_TILE","wall_mat":"IB3_PH_CHIPPED_CONCRETE","assets":["PH_TRANSIT_BENCH_ADAPTED","INT_MAILBOX_BANK","INT_BUILDING_DIRECTORY","STREET_PLANTER","STREET_NEWSPAPER_BOX","ARCH_SERVICE_DOOR","INT_VENDING_MACHINE"],"placements":[("PH_TRANSIT_BENCH_ADAPTED",(-2.4,1.8,0),.85,0),("INT_MAILBOX_BANK",(-3.8,2.6,0),1.3,0),("INT_BUILDING_DIRECTORY",(3.7,2.5,0),1,0),("STREET_PLANTER",(3,-2.6,0),1,0),("STREET_NEWSPAPER_BOX",(-3,-2.8,0),.8,0),("ARCH_SERVICE_DOOR",(0,3.75,0),1,0),("INT_VENDING_MACHINE",(3.5,.5,0),.9,0)],"portals":[door("NAV_PORTAL_LOBBY_V3",0,-3.9,"DOOR_LOBBY_V3"),door("NAV_PORTAL_ELEVATOR_V3",0,3.8,"DOOR_ELEVATOR_V3")],"interactions":[inter("INTERACT_LOBBY_PANEL",1.4,1.1,3.2,"panel"),inter("INTERACT_LOBBY_BENCH",-2.4,.7,1.8,"sit")],"colliders":[coll("COLLIDER_MAILBOX",[-3.8,1,2.6],[.3,1,1.2]),coll("COLLIDER_BENCH",[-2.4,.5,1.8],[1.2,.5,.45])],"staging":[{"id":"MARK_LOBBY_WAIT","position":[0,0,0]},{"id":"MARK_LOBBY_PANEL","position":[.8,0,2.8]}],"camera":(8,-11,6),"target":(0,.5,1.1)},
 {"id":"location_transit_pocket_v3","size":(12,7,3.2),"floor_mat":"IB3_PH_ASPHALT","wall_mat":"IB3_DARK_TEAL","assets":["PH_TRANSIT_BENCH_ADAPTED","PH_STREET_LAMP_ADAPTED","STREET_TRANSIT_MAP","STREET_NEWSPAPER_BOX","STREET_TRASH_RECYCLING","STREET_PLANTER","STREET_TREE_GUARD","STREET_BOLLARD_SET","STREET_SIGNPOST"],"placements":[("PH_TRANSIT_BENCH_ADAPTED",(-1,1,0),1,0),("PH_STREET_LAMP_ADAPTED",(-5,1.8,0),1,0),("STREET_TRANSIT_MAP",(3,2.4,0),1,0),("STREET_NEWSPAPER_BOX",(4.4,1.5,0),.8,0),("STREET_TRASH_RECYCLING",(-4,-1.8,0),.8,0),("STREET_PLANTER",(4.5,-1.9,0),1,0),("STREET_BOLLARD_SET",(0,-2.5,0),1,0),("STREET_SIGNPOST",(5,2.5,0),1,0)],"portals":[door("NAV_PORTAL_TRANSIT_EAST",5.9,0,None),door("NAV_PORTAL_TRANSIT_WEST",-5.9,0,None)],"interactions":[inter("INTERACT_TRANSIT_WAIT",-1,0,1,"wait"),inter("INTERACT_TRANSIT_MAP",3,1,2.4,"panel")],"colliders":[coll("COLLIDER_TRANSIT_BENCH",[-1,.5,1],[1.3,.5,.45]),coll("COLLIDER_TRANSIT_MAP",[3,1,2.4],[.45,1,.4])],"staging":[{"id":"MARK_TRANSIT_WAIT_A","position":[-1,0,-.3]},{"id":"MARK_TRANSIT_WAIT_B","position":[1,0,-.3]}],"camera":(10,-12,6),"target":(0,0,1)},
 {"id":"location_laundromat_v3","size":(11,8,3.5),"floor_mat":"IB3_PH_CHIPPED_CONCRETE","wall_mat":"IB3_TEAL","assets":["INT_WASHER","INT_DRYER_BANK","INT_LAUNDRY_CART","INT_FOLDING_TABLE","INT_VENDING_MACHINE","PH_PLASTIC_CRATE_ADAPTED","INT_COUNTER_STOOL","STREET_FOLDING_BARRIER"],"placements":[("INT_WASHER",(-4,2.7,0),1.2,0),("INT_WASHER",(-2.2,2.7,0),1.2,0),("INT_DRYER_BANK",(1,2.7,0),1.3,0),("INT_DRYER_BANK",(3.4,2.7,0),1.3,0),("INT_LAUNDRY_CART",(-2,0,0),1,0),("INT_FOLDING_TABLE",(1,0,0),1.4,0),("INT_VENDING_MACHINE",(4.5,-2.4,0),1,0),("INT_COUNTER_STOOL",(2,-1,0),1,0),("PH_PLASTIC_CRATE_ADAPTED",(-4,-2.3,0),.9,0)],"portals":[door("NAV_PORTAL_LAUNDROMAT",0,-3.9,"DOOR_LAUNDROMAT")],"interactions":[inter("INTERACT_WASHER",-4,1,2.7,"panel"),inter("INTERACT_FOLDING_TABLE",1,1,0,"counter")],"colliders":[coll("COLLIDER_WASHER_BANK",[-2.4,.8,2.7],[2.4,.8,.7]),coll("COLLIDER_DRYER_BANK",[2.2,.8,2.7],[2.4,.8,.7]),coll("COLLIDER_FOLD_TABLE",[1,.7,0],[1.2,.7,.7])],"staging":[{"id":"MARK_LAUNDRY_WAIT","position":[-2,0,-1.5]},{"id":"MARK_LAUNDRY_FOLD","position":[1,0,-1]}],"camera":(11,-12,7),"target":(0,.5,1.1)},
 {"id":"location_maintenance_workshop_v3","size":(12,9,3.8),"floor_mat":"IB3_PH_METAL_GRATE","wall_mat":"IB3_PH_GREEN_RUST","assets":["PH_TOOL_CHEST_ADAPTED","PH_PLASTIC_CRATE_ADAPTED","INT_WORKBENCH","INT_TOOL_WALL","INT_LOCKER_BANK","ARCH_CONDUIT_BUNDLE","ARCH_VENT_UNIT","STREET_UTILITY_CABINET","STREET_WORK_ZONE"],"placements":[("PH_TOOL_CHEST_ADAPTED",(-3.8,2.6,0),1,0),("PH_PLASTIC_CRATE_ADAPTED",(4,-2.8,0),1.1,0),("INT_WORKBENCH",(0,2.6,0),1.7,0),("INT_TOOL_WALL",(3.5,2.7,0),1.4,0),("INT_LOCKER_BANK",(-4,-.5,0),1.3,0),("ARCH_CONDUIT_BUNDLE",(5.2,2,0),1.5,0),("ARCH_VENT_UNIT",(3,-2.5,0),1.2,0),("STREET_UTILITY_CABINET",(-1,-2.8,0),1,0),("STREET_WORK_ZONE",(-4,-3,0),1,0)],"portals":[door("NAV_PORTAL_WORKSHOP",0,-4.4,"DOOR_WORKSHOP")],"interactions":[inter("INTERACT_TOOL_CHEST",-3.8,1,2.6,"pickup"),inter("INTERACT_WORKBENCH",0,1,2.6,"counter")],"colliders":[coll("COLLIDER_WORKBENCH",[0,.8,2.6],[1.8,.8,.8]),coll("COLLIDER_TOOL_CHEST",[-3.8,.75,2.6],[.8,.75,.55]),coll("COLLIDER_LOCKERS",[-4,1,-.5],[.6,1,1.5])],"staging":[{"id":"MARK_WORKBENCH","position":[0,0,1.3]},{"id":"MARK_WORKSHOP_WIDE","position":[0,0,-1.5]}],"camera":(12,-13,7.5),"target":(0,.6,1.2)},
 ]
 for spec in result:
  _,depth,_=spec["size"];prefix=spec["id"].upper()
  spec["staging"].extend([
   {"id":"MARK_"+prefix+"_ENTRY","position":[0,0,-depth/2+1.0]},
   {"id":"MARK_"+prefix+"_CAMERA_SAFE","position":[0,0,depth/2-1.0]},
  ])
 return result

def registry_point(item):
 return {"id":item["id"],"node":item["id"],"position":item["position"]}|{k:v for k,v in item.items() if k not in {"id","position"}}

def update_registry(sidecars):
 data=json.loads(REGISTRY.read_text());by={m["module_id"]:m for m in data["modules"]};spec_by={s["id"]:s for s in specs()}
 for side in sidecars:
  spec=spec_by[side["module_id"]];w,d,h=spec["size"]
  by[side["module_id"]]={
   "schema_version":1,"module_id":side["module_id"],"asset":side["asset"],"source_blend":side["source_blend"],"category":side["category"],"version":side["version"],"quality_tier":side["quality_tier"],
   "bounds":{"min":[-w/2,-.25,-d/2],"max":[w/2,h,d/2]},
   "sockets":[registry_point(item) for item in side["portals"]],
   "staging_marks":[registry_point(item) for item in side["staging_marks"]],
   "camera_anchors":[registry_point(item) for item in side["camera_anchors"]],
   "interactions":[registry_point(item) for item in side["interactions"]],
   "cutaway_groups":[],
   "collision_groups":[registry_point({"id":item["id"],"position":item["center"],"shape":item["shape"],"half_extents":item["half_extents"],"role":item["role"]}) for item in side["colliders"]],
   "glb_sha256":side["glb_sha256"],"preview":side["preview"],"tags":["recurring_location","cc0_adapted","navigation_ready"],"provenance":side["provenance"]}
 data["modules"]=sorted(by.values(),key=lambda x:x["module_id"]);data["module_count"]=len(data["modules"]);REGISTRY.write_text(json.dumps(data,indent=2)+"\n")

def main():
 for p in [KIT.parent,KIT_GLB.parent,CATALOG.parent,LOCATION_SOURCE,LOCATION_RUNTIME,REFERENCE/"hero_previews"]:p.mkdir(parents=True,exist_ok=True)
 catalog=build_library();sidecars=[build_location(s) for s in specs()];update_registry(sidecars);print(json.dumps({"assets":len(catalog),"materials":len(PALETTE)+len(TEXTURE_MATERIALS),"hdris":len(HDRI_PRESETS),"locations":len(sidecars),"registry_modules":json.loads(REGISTRY.read_text())["module_count"]}))

if __name__=="__main__":main()
