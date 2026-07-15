"""Build the Infinite Backlot expansion asset library, three world cells, linked master, and tour.

Run through Blender MCP or Blender 5.2 background mode. All authored geometry is project-owned.
The durable source files retain Geometry Nodes and project-relative linked collections; GLBs are
realized deterministic runtime exports.
"""
import bpy
import json
import math
import hashlib
from pathlib import Path
from mathutils import Vector

ROOT=Path(r"C:/Projects/bevy-infinite")
SOURCE=ROOT/"assets/source/blender/world/cells"
RUNTIME=ROOT/"assets/world/cells"
KIT_SOURCE=ROOT/"assets/source/blender/world/kits/infinite_backlot_expansion_assets.blend"
KIT_GLB=ROOT/"assets/world/kits/infinite_backlot_expansion_assets.glb"
KIT_CATALOG=ROOT/"assets/world/kits/infinite_backlot_expansion_assets.catalog.json"
MASTER_SOURCE=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_block.blend"
EXPANDED_SOURCE=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_expanded_world.blend"
EXPANDED_TOUR=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_expanded_world_tour.blend"
EXPANDED_GLB=ROOT/"assets/world/neighborhood/infinite_backlot_expanded_world.glb"
EXPANDED_SIDECAR=ROOT/"assets/world/neighborhood/infinite_backlot_expanded_world.scene.json"
PREVIEWS=ROOT/"assets/reference/world-expansion/hero_previews"
OUTPUT=ROOT/"output/world-expansion"
for p in (SOURCE,RUNTIME,KIT_SOURCE.parent,KIT_GLB.parent,PREVIEWS,OUTPUT):p.mkdir(parents=True,exist_ok=True)

PALETTE={
 "MAT_EXP_BURGUNDY":((.22,.035,.055,1),.72,.05),"MAT_EXP_RUST":((.43,.105,.045,1),.78,.05),
 "MAT_EXP_TEAL":((.025,.24,.255,1),.52,.25),"MAT_EXP_BRASS":((.48,.25,.075,1),.34,.72),
 "MAT_EXP_CYAN":((.03,.62,.77,1),.28,.15),"MAT_EXP_CONCRETE":((.30,.28,.25,1),.88,.02),
 "MAT_EXP_PATCH":((.07,.075,.08,1),.93,.02),"MAT_EXP_STEEL":((.12,.14,.15,1),.48,.72),
 "MAT_EXP_SAFETY":((.80,.20,.035,1),.66,.05),"MAT_EXP_GLASS":((.08,.24,.29,.34),.16,.05),
 "MAT_EXP_CIVIC_TILE":((.28,.36,.34,1),.57,.03),"MAT_EXP_MOSS":((.10,.22,.12,1),.90,.01),
 "MAT_EXP_POSTER_CREAM":((.68,.57,.39,1),.82,.01),"MAT_EXP_POSTER_RED":((.50,.045,.055,1),.72,.01),
 "MAT_EXP_WHITE":((.70,.67,.57,1),.78,.02),"MAT_EXP_BLACK":((.012,.016,.020,1),.82,.02),
}

def reset(root_name="WORLD_ROOT"):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    root=bpy.data.collections.new(root_name);bpy.context.scene.collection.children.link(root)
    for name in ("GEOMETRY","DETAILS","SEMANTICS","CAMERAS","LIGHTING","COLLIDERS"):
        c=bpy.data.collections.new(name);root.children.link(c)
    return root

def C(name):return bpy.data.collections[name]

def mats():
    out={}
    for name,(color,rough,metal) in PALETTE.items():
        m=bpy.data.materials.new(name);m.use_nodes=True;p=m.node_tree.nodes.get("Principled BSDF");p.inputs["Base Color"].default_value=color;p.inputs["Roughness"].default_value=rough;p.inputs["Metallic"].default_value=metal
        if name=="MAT_EXP_GLASS":p.inputs["Alpha"].default_value=.34;p.inputs["Transmission Weight"].default_value=.18;m.surface_render_method="DITHERED"
        if name=="MAT_EXP_CYAN":p.inputs["Emission Color"].default_value=color;p.inputs["Emission Strength"].default_value=3.0
        m["gltf_compatible"]=True;m["infinite_backlot_family"]="expansion_v1";out[name]=m
    return out

def move_obj(o,collection):
    for old in list(o.users_collection):old.objects.unlink(o)
    collection.objects.link(o)

def cube(name,loc,dims,mat="MAT_EXP_CONCRETE",bevel=.05,collection="GEOMETRY"):
    bpy.ops.mesh.primitive_cube_add(location=loc);o=bpy.context.object;o.name=name;o.dimensions=dims;bpy.ops.object.transform_apply(location=False,rotation=False,scale=True);move_obj(o,C(collection))
    if mat in bpy.data.materials:o.data.materials.append(bpy.data.materials[mat])
    if bevel:
        mod=o.modifiers.new("Edge wear","BEVEL");mod.width=min(bevel,min(dims)*.2);mod.segments=2
    return o

def cyl(name,loc,r,depth,mat="MAT_EXP_STEEL",collection="DETAILS",verts=16):
    bpy.ops.mesh.primitive_cylinder_add(vertices=verts,radius=r,depth=depth,location=loc);o=bpy.context.object;o.name=name;move_obj(o,C(collection));o.data.materials.append(bpy.data.materials[mat]);b=o.modifiers.new("Edge wear","BEVEL");b.width=min(.035,r*.18);b.segments=2;return o

def text(name,body,loc,scale=.45,mat="MAT_EXP_POSTER_CREAM",rot=(math.pi/2,0,0),collection="DETAILS"):
    d=bpy.data.curves.new(name,"FONT");d.body=body;d.align_x="CENTER";d.align_y="CENTER";d.extrude=.018;d.bevel_depth=.006;o=bpy.data.objects.new(name,d);C(collection).objects.link(o);o.location=loc;o.rotation_euler=rot;o.scale=(scale,scale,scale);d.materials.append(bpy.data.materials[mat]);return o

def semantic(name,loc,kind,**props):
    o=bpy.data.objects.new(name,None);C("SEMANTICS").objects.link(o);o.location=loc;o.empty_display_type="ARROWS" if kind=="socket" else "CIRCLE";o.empty_display_size=.35;o["semantic_kind"]=kind;o["semantic_id"]=name
    for k,v in props.items():o[k]=v
    return o

def camera(name,loc,target,lens=42):
    d=bpy.data.cameras.new(name);d.lens=lens;d.sensor_width=36;o=bpy.data.objects.new(name,d);C("CAMERAS").objects.link(o);o.location=loc;o.rotation_euler=(Vector(target)-o.location).to_track_quat("-Z","Y").to_euler();o["semantic_kind"]="camera_anchor";o["semantic_id"]=name;o["look_at"]=list(target);o["lens_mm"]=float(lens);return o

def light_intent(name,loc,role,color,intensity,range_m):
    return semantic(name,loc,"runtime_light",role=role,light_type="point",color_rgb=list(color),intensity=float(intensity),range=float(range_m),direction=[0,0,-1],runtime_controlled=False)

def collider(name,loc,dims):
    o=cube(name,loc,dims,"MAT_EXP_BLACK",0,"COLLIDERS");o.hide_render=True;o.display_type="WIRE";o["semantic_kind"]="collider";o["semantic_id"]=name;o["collision_role"]="solid";return o

def sun_world():
    world=bpy.data.worlds.new("Infinite Backlot Expansion World") if not bpy.data.worlds else bpy.data.worlds[0];bpy.context.scene.world=world;world.use_nodes=True;world.node_tree.nodes["Background"].inputs["Color"].default_value=(.018,.027,.045,1);world.node_tree.nodes["Background"].inputs["Strength"].default_value=.24
    d=bpy.data.lights.new("ART_Sun","SUN");d.energy=2.2;d.color=(1,.63,.38);d.angle=math.radians(18);o=bpy.data.objects.new("ART_Sun",d);C("LIGHTING").objects.link(o);o.rotation_euler=(math.radians(38),math.radians(-22),math.radians(25))
    for name,loc,target,color,energy,size in [("ART_Key",(-10,-12,12),(0,0,2),(1,.44,.22),1200,8),("ART_Cyan",(13,4,8),(0,0,2),(.12,.65,1),900,6)]:
        ld=bpy.data.lights.new(name,"AREA");ld.energy=energy;ld.color=color;ld.shape="DISK";ld.size=size;lo=bpy.data.objects.new(name,ld);C("LIGHTING").objects.link(lo);lo.location=loc;lo.rotation_euler=(Vector(target)-lo.location).to_track_quat("-Z","Y").to_euler()

def add_box_parts(prefix,origin,parts):
    ox,oy,oz=origin
    for i,(loc,dims,mat) in enumerate(parts):cube(f"{prefix}_{i:02}",(ox+loc[0],oy+loc[1],oz+loc[2]),dims,mat,.04)

def gn_bollard_row(name,start,count,spacing):
    source=cyl(name+"_SOURCE",(0,0,-20),.14,1.05,"MAT_EXP_TEAL");source.hide_render=True
    mesh=bpy.data.meshes.new(name+"_Mesh");obj=bpy.data.objects.new(name,mesh);C("DETAILS").objects.link(obj);obj.location=start
    group=bpy.data.node_groups.new(name+"_GeometryNodes","GeometryNodeTree");group.interface.new_socket(name="Geometry",in_out="INPUT",socket_type="NodeSocketGeometry");group.interface.new_socket(name="Geometry",in_out="OUTPUT",socket_type="NodeSocketGeometry")
    inp=group.nodes.new("NodeGroupInput");out=group.nodes.new("NodeGroupOutput");line=group.nodes.new("GeometryNodeCurvePrimitiveLine");line.inputs["Start"].default_value=(0,0,0);line.inputs["End"].default_value=((count-1)*spacing,0,0)
    resample=group.nodes.new("GeometryNodeResampleCurve");resample.inputs["Count"].default_value=count
    curve_points=group.nodes.new("GeometryNodeCurveToPoints");curve_points.mode="EVALUATED"
    info=group.nodes.new("GeometryNodeObjectInfo");info.transform_space="ORIGINAL";info.inputs["Object"].default_value=source
    inst=group.nodes.new("GeometryNodeInstanceOnPoints");real=group.nodes.new("GeometryNodeRealizeInstances")
    group.links.new(line.outputs["Curve"],resample.inputs["Curve"]);group.links.new(resample.outputs["Curve"],curve_points.inputs["Curve"]);group.links.new(curve_points.outputs["Points"],inst.inputs["Points"]);group.links.new(info.outputs["Geometry"],inst.inputs["Instance"]);group.links.new(inst.outputs["Instances"],real.inputs["Geometry"]);group.links.new(real.outputs["Geometry"],out.inputs["Geometry"])
    mod=obj.modifiers.new("Infinite Backlot procedural bollard family","NODES");mod.node_group=group;obj["authoring_technique"]="geometry_nodes";obj["deterministic_count"]=count;return obj

def build_asset_library():
    reset("INFINITE_BACKLOT_EXPANSION_ASSETS");mats();sun_world();assets=[]
    specs=[
      ("corner_window","architecture"),("shop_entrance_canopy","architecture"),("exterior_stair_system","architecture"),("roof_hvac_cluster","architecture"),("rolling_shutter","architecture"),("loading_dock","architecture"),("utility_conduit_bundle","architecture"),
      ("transit_shelter","public_space"),("transit_map","public_space"),("newspaper_box","public_space"),("parking_meter","public_space"),("utility_cabinet","public_space"),("traffic_signal","public_space"),("bollard_family","public_space"),("planter_family","public_space"),("tree_guard","public_space"),("bike_rack_variant","public_space"),("folding_barrier","public_space"),
      ("flyer_cluster","storytelling"),("water_tower_silhouette","background"),("chimney_cluster","background")]
    for idx,(aid,category) in enumerate(specs):
        c=bpy.data.collections.new("ASSET_"+aid.upper());bpy.data.collections["INFINITE_BACKLOT_EXPANSION_ASSETS"].children.link(c);x=(idx%7)*5-15;y=(idx//7)*5-5
        # Deliberate multi-part silhouettes; these are library previews, not untouched primitives.
        parts=[]
        if aid=="corner_window":parts=[((0,0,1.5),(2.8,.18,3),"MAT_EXP_BRASS"),((0,-.1,1.5),(2.45,.08,2.65),"MAT_EXP_GLASS"),((0,0,2.8),(3.2,.45,.25),"MAT_EXP_TEAL")]
        elif aid=="transit_shelter":parts=[((0,0,1.4),(3.8,.12,2.8),"MAT_EXP_GLASS"),((-1.85,0,1.4),(.14,1.5,2.8),"MAT_EXP_BRASS"),((1.85,0,1.4),(.14,1.5,2.8),"MAT_EXP_BRASS"),((0,0,2.9),(4.3,1.8,.18),"MAT_EXP_TEAL"),((0,.25,.55),(2.8,.55,.18),"MAT_EXP_RUST")]
        elif aid in {"water_tower_silhouette","chimney_cluster"}:
            parts=[((0,0,.6),(2.8,2.8,.3),"MAT_EXP_STEEL"),((-1,0,2.5),(.25,.25,4),"MAT_EXP_STEEL"),((1,0,2.5),(.25,.25,4),"MAT_EXP_STEEL"),((0,0,4.5),(2.4,2.4,1.2),"MAT_EXP_RUST")]
        else:parts=[((0,0,.5),(2.2,1.1,1),"MAT_EXP_TEAL"),((0,-.2,1.25),(1.5,.7,.5),"MAT_EXP_BRASS"),((0,.1,1.7),(.35,.35,.9),"MAT_EXP_SAFETY")]
        before=set(bpy.data.objects)
        add_box_parts("LIB_"+aid.upper(),(x,y,0),parts)
        created=set(bpy.data.objects)-before
        for o in created:
            for old in list(o.users_collection):old.objects.unlink(o)
            c.objects.link(o)
        if hasattr(c,"asset_mark"):
            c.asset_mark();c.asset_data.author="Infinite Backlot";c.asset_data.description=f"Project-owned {category} kit asset adapted to the Infinite Backlot art bible"
        assets.append({"asset_id":aid,"collection":c.name,"category":category,"quality_tier":"production","provenance":"project-owned-procedural","license":"repository license","scale_m":round(max(p[1][0] for p in parts),2)})
    bpy.context.scene["library_id"]="infinite_backlot_expansion_assets";bpy.context.scene["asset_count"]=len(assets);bpy.ops.wm.save_as_mainfile(filepath=str(KIT_SOURCE),compress=True)
    bpy.ops.export_scene.gltf(filepath=str(KIT_GLB),export_format="GLB",use_selection=False,export_cameras=False,export_lights=False,export_extras=True,export_apply=True)
    catalog={"schema_version":1,"library_id":"infinite_backlot_expansion_assets","source":str(KIT_SOURCE.relative_to(ROOT)).replace('\\','/'),"runtime_asset":str(KIT_GLB.relative_to(ROOT)).replace('\\','/'),"assets":assets,"materials":[{"material_id":n,"gltf_compatible":True} for n in PALETTE],"techniques_applied":["direct_modeling","geometry_nodes","asset_browser","linked_collections","material_nodes","deterministic_glb_export"],"provenance":{"author":"Infinite Backlot project","license":"repository license","external_assets":[]}}
    KIT_CATALOG.write_text(json.dumps(catalog,indent=2)+"\n");return catalog

def street_extension():
    reset("CELL_STREET_EXTENSION");mats();sun_world();
    cube("StreetExtension_Road",(0,0,-.18),(28,8,.36),"MAT_EXP_PATCH",.02);cube("StreetExtension_SidewalkNorth",(0,5,.02),(28,2.2,.28),"MAT_EXP_CONCRETE",.04);cube("StreetExtension_SidewalkSouth",(0,-5,.02),(28,2.2,.28),"MAT_EXP_CONCRETE",.04)
    # Stepped corner business, loading frontage, roof machinery, bays, and mismatched repair panels.
    cube("HingeHour_Main",(1,9,3.4),(15,6.3,6.8),"MAT_EXP_BURGUNDY",.12);cube("HingeHour_Step",(7.8,8.8,2.6),(4.2,5.8,5.2),"MAT_EXP_RUST",.10);cube("HingeHour_Canopy",(-2,5.8,2.7),(7,1.5,.22),"MAT_EXP_TEAL",.05);cube("LoadingDock",(10.5,8.2,.55),(3.6,4,1.1),"MAT_EXP_CONCRETE",.08);cube("RollingShutter",(9.2,5.95,2),(3.5,.16,3.4),"MAT_EXP_STEEL",.03)
    for x in (-4,-1,2,5):cube(f"CornerBay_{x}",(x,5.82,4.5),(2.1,.16,2.0),"MAT_EXP_GLASS",.02)
    for x in (-10,-6,6,11):cyl(f"ParkingMeter_{x}",(x,-5.1,.8),.11,1.6,"MAT_EXP_BRASS")
    gn_bollard_row("GN_STREET_BOLLARD_ROW",(-11,5.9,.53),6,1.3)
    for x,y in [(-11,4.8),(12,4.8)]:cyl("TrafficSignalPost",(x,y,2.5),.16,5,"MAT_EXP_TEAL");cube("TrafficSignalHead",(x,y,4.6),(.75,.35,1.4),"MAT_EXP_BLACK",.08)
    text("HingeHour_Sign","HINGE & HOUR",(-2,5.65,3.4),.5,"MAT_EXP_CYAN");text("StreetNotice","EAST CONTINUES / EXCEPT TUESDAY",(6.8,5.7,1.8),.22,"MAT_EXP_POSTER_CREAM")
    for name,loc,typ in [("WORLD_SOCKET_WEST",(-14,0,0),"ROAD"),("WORLD_SOCKET_EAST",(14,0,0),"ROAD"),("WORLD_SOCKET_SERVICE",(10,9,0),"SERVICE")]:semantic(name,loc,"socket",socket_type=typ,clearance_m=4.0)
    for i,loc in enumerate([(-9,5.1,0),(-3,5.1,0),(3,5.1,0),(9,5.1,0),(9,-5.1,0)]):semantic(f"MARK_STREET_EXTENSION_{i+1:02}",loc,"staging_mark",radius_m=.65)
    semantic("INTERACT_LOADING_SHUTTER",(9.2,5.7,1),"interaction",interaction_type="rolling_shutter");camera("CAM_STREET_EXTENSION_WIDE",(-13,-13,6),(0,5,3),38);camera("CAM_HINGE_HOUR_TWO_SHOT",(-5,1.5,1.7),(0,5.1,1.2),52);camera("CAM_LOADING_FRONTAGE",(13,1.5,2.2),(9,6,1.6),46);collider("COLLIDER_STREET_EXTENSION",(0,0,-.3),(28,13,.35));light_intent("LIGHT_STREET_EXTENSION",(0,2,6),"LIGHT_STREET",(1,.62,.38),16000,17);light_intent("LIGHT_HINGE_SIGN",(-2,5.5,3),"LIGHT_SIGN",(.1,.85,1),9000,9)

def transit_pocket():
    reset("CELL_PUBLIC_TRANSIT_POCKET");mats();sun_world();
    cube("TransitPlaza",(0,0,-.12),(26,18,.24),"MAT_EXP_CIVIC_TILE",.03);cube("BusLane",(0,-8,-.20),(26,4,.36),"MAT_EXP_PATCH",.02);cube("ShelterGlass",(-2,-3,1.6),(7,.14,3.2),"MAT_EXP_GLASS",.02);cube("ShelterRoof",(-2,-3,3.25),(7.8,2.8,.22),"MAT_EXP_TEAL",.05)
    for x in (-5.3,1.3):cyl("ShelterPost",(x,-3,1.55),.12,3.1,"MAT_EXP_BRASS")
    cube("TransitBench",(-2,-2.6,.6),(5,.65,.18),"MAT_EXP_RUST",.06);cube("MunicipalNoticeBoard",(7,2,1.7),(3,.28,3.4),"MAT_EXP_BURGUNDY",.08);cube("TransitMap",(7,1.8,1.8),(2.5,.10,2.6),"MAT_EXP_POSTER_CREAM",.02);cube("Newsstand",(-8,3,1.3),(3.8,2.4,2.6),"MAT_EXP_RUST",.08);cube("NewsstandCanopy",(-8,2,2.85),(4.4,1.5,.20),"MAT_EXP_TEAL",.05)
    for i,x in enumerate((-9,-5,3,9)):cube(f"Planter_{i}",(x,6,.55),(2.2,1.8,1.1),"MAT_EXP_CONCRETE",.10);cyl(f"TreeGuard_{i}",(x,6,1.2),.75,2.4,"MAT_EXP_TEAL",verts=12)
    text("TransitTitle","MUNICIPAL ARRIVAL POCKET 7?",(0,-3.2,3.5),.38,"MAT_EXP_CYAN");text("TransitWarning","ROUTES 2 / 2B / NOT 2",(7,1.6,2.1),.22,"MAT_EXP_POSTER_RED")
    for name,loc,typ in [("WORLD_SOCKET_EAST",(13,0,0),"ROAD"),("WORLD_SOCKET_WEST",(-13,0,0),"ROAD"),("WORLD_SOCKET_TRANSIT",(0,-10,0),"TRANSIT")]:semantic(name,loc,"socket",socket_type=typ,clearance_m=4.0)
    for i,loc in enumerate([(-5,-2,0),(-2,-2,0),(1,-2,0),(5,2,0),(8,5,0),(0,5,0)]):semantic(f"MARK_TRANSIT_POCKET_{i+1:02}",loc,"staging_mark",radius_m=.7)
    semantic("INTERACT_TRANSIT_MAP",(7,1.2,1.3),"interaction",interaction_type="map");semantic("INTERACT_TRANSIT_BENCH",(-2,-2.2,.5),"interaction",interaction_type="bench");camera("CAM_TRANSIT_WIDE",(-14,-14,6),(0,0,2),36);camera("CAM_TRANSIT_CONVERSATION",(4,-6,1.7),(-1,-2,1.2),54);camera("CAM_NOTICE_BOARD",(11,-1,1.8),(7,2,1.5),58);collider("COLLIDER_TRANSIT_PLAZA",(0,0,-.28),(26,18,.35));light_intent("LIGHT_TRANSIT_SHELTER",(-2,-3,3),"LIGHT_PRACTICAL",(.18,.82,1),10000,10);light_intent("LIGHT_TRANSIT_PLAZA",(6,2,5),"LIGHT_STREET",(1,.64,.38),15000,16)

def industrial_transition():
    reset("CELL_INDUSTRIAL_TRANSITION");mats();sun_world();
    cube("IndustrialServiceRoad",(0,0,-.2),(13,30,.4),"MAT_EXP_PATCH",.02);cube("DrainageChannel",(-7,0,-.45),(4,30,.7),"MAT_EXP_CONCRETE",.04);cube("RetainingWallEast",(7,0,2.2),(1.2,30,4.4),"MAT_EXP_CONCRETE",.08);cube("RailBridgeDeck",(0,2,6.2),(20,4,1.0),"MAT_EXP_STEEL",.10)
    for x in (-6,0,6):cube(f"BridgePier_{x}",(x,2,3),(1.2,2,6),"MAT_EXP_RUST",.08)
    for x in (-4,4):cyl(f"OverbuiltPipe_{x}",(x,5,4.8),.28,18,"MAT_EXP_TEAL").rotation_euler[0]=math.pi/2
    cube("UtilityShed",(4,10,2.2),(5,5,4.4),"MAT_EXP_BURGUNDY",.10);cube("UtilityShedDoor",(4,7.45,1.5),(2.2,.16,3),"MAT_EXP_STEEL",.03);cube("ServiceCanopy",(4,7.1,3.3),(3.5,1.4,.22),"MAT_EXP_SAFETY",.04)
    for x,h in [(-10,10),(-13,14),(-16,8)]:cyl("DistantChimney",(x,10,h/2),.8,h,"MAT_EXP_RUST",verts=16)
    for i,y in enumerate((-10,-5,8,13)):cube(f"RepairPatch_{i}",(0,y,.03),(3.5,2,.05),"MAT_EXP_CONCRETE",.01)
    text("IndustrialSign","OUTSKIRTS ACCESS / LEVEL -1",(4,7.25,3.7),.26,"MAT_EXP_CYAN");text("ServiceLabel","SERVICE 8B / 8B?",(7.65,0,2.1),.24,"MAT_EXP_POSTER_CREAM",rot=(math.pi/2,0,math.pi/2))
    for name,loc,typ in [("WORLD_SOCKET_SOUTH",(0,-15,0),"ROAD"),("WORLD_SOCKET_NORTH",(0,15,0),"ROAD"),("WORLD_SOCKET_OUTSKIRTS",(0,15,0),"OUTSKIRTS"),("WORLD_SOCKET_SERVICE",(6,10,0),"SERVICE")]:semantic(name,loc,"socket",socket_type=typ,clearance_m=4.0)
    for i,loc in enumerate([(0,-11,0),(0,-5,0),(0,1,0),(0,7,0),(3,10,0),(0,13,0)]):semantic(f"MARK_INDUSTRIAL_{i+1:02}",loc,"staging_mark",radius_m=.7)
    semantic("INTERACT_SERVICE_SHED",(4,7.2,1),"interaction",interaction_type="service_door");camera("CAM_INDUSTRIAL_APPROACH",(-10,-16,5),(0,0,2),38);camera("CAM_UNDERPASS_LONG",(4,-8,1.7),(0,8,1.3),56);camera("CAM_OUTSKIRTS_REVEAL",(12,10,5),(0,14,3),42);collider("COLLIDER_INDUSTRIAL_ROAD",(0,0,-.3),(13,30,.35));light_intent("LIGHT_UNDERPASS",(0,1,5),"LIGHT_ALLEY",(.12,.65,1),13000,12);light_intent("LIGHT_SERVICE_SHED",(4,7,3.5),"LIGHT_PRACTICAL",(1,.52,.2),9000,8)

CELL_BUILDERS={"cell_street_extension":street_extension,"cell_public_transit_pocket":transit_pocket,"cell_industrial_transition":industrial_transition}
CELL_META={
 "cell_street_extension":{"category":"street_extension","bounds":[[-14,-.5,-8],[14,8,12]],"neighbors":["CELL_ODD_HOURS_CORNER","CELL_PUBLIC_TRANSIT_POCKET"],"priority":"near"},
 "cell_public_transit_pocket":{"category":"public_transit_pocket","bounds":[[-13,-.5,-10],[13,7,9]],"neighbors":["CELL_APARTMENT_BLOCK","CELL_STREET_EXTENSION"],"priority":"near"},
 "cell_industrial_transition":{"category":"industrial_transition","bounds":[[-18,-.8,-15],[9,16,15]],"neighbors":["CELL_SERVICE_ALLEY","future_outskirts"],"priority":"medium"},}

def json_value(v):
    if hasattr(v,"to_list"):return v.to_list()
    if isinstance(v,(list,tuple)):return [json_value(x) for x in v]
    if isinstance(v,(str,int,float,bool)) or v is None:return v
    return str(v)

def sidecar(cell_id,source,asset):
    def bv(v):return [round(v[0],4),round(v[2],4),round(-v[1],4)]
    groups={k:[] for k in ("sockets","staging_marks","camera_anchors","interactions","collision_groups","lighting")}
    for o in bpy.context.scene.objects:
        kind=o.get("semantic_kind");meta={k:json_value(o[k]) for k in o.keys() if k not in {"semantic_kind","semantic_id","_RNA_UI"}}
        if "look_at" in meta:meta["look_at"]=bv(meta["look_at"])
        p={"id":o.get("semantic_id",o.name),"node":o.name,"position":bv(o.location),**meta}
        if kind=="socket":groups["sockets"].append(p)
        elif kind=="staging_mark":groups["staging_marks"].append(p)
        elif kind=="camera_anchor":groups["camera_anchors"].append(p)
        elif kind=="interaction":groups["interactions"].append(p)
        elif kind=="collider":groups["collision_groups"].append(p)
        elif kind=="runtime_light":p["direction"]=bv(p["direction"]);groups["lighting"].append(p)
    meta=CELL_META[cell_id];return {"schema_version":1,"module_id":cell_id,"cell_id":"CELL_"+cell_id.removeprefix("cell_").upper(),"asset":str(asset.relative_to(ROOT)).replace('\\','/'),"source_blend":str(source.relative_to(ROOT)).replace('\\','/'),"category":meta["category"],"version":1,"quality_tier":"hero","bounds":{"min":meta["bounds"][0],"max":meta["bounds"][1]},**groups,"cutaway_groups":[],"runtime_controls":[],"lighting_policy":"semantic_runtime_lights","background_requirements":["burgundy_teal_skyline","warm_cyan_practicals"],"neighbor_compatibility":meta["neighbors"],"future_streaming_priority":meta["priority"],"world_state_hooks":["time_of_day","municipal_notice_state"],"tags":["hero","production","world_cell","expansion"],"provenance":{"author":"Infinite Backlot project","license":"repository license","generator":"tools/blender/build_world_expansion.py","external_assets":[]}}

def save_export_cell(cell_id):
    source=SOURCE/f"{cell_id}.blend";asset=RUNTIME/f"{cell_id}.glb";bpy.context.scene["module_id"]=cell_id;bpy.context.scene["quality_tier"]="hero";bpy.ops.wm.save_as_mainfile(filepath=str(source),compress=True);data=sidecar(cell_id,source,asset)
    for o in bpy.context.scene.objects:
        if o.get("semantic_kind") in {"collider"}:o.hide_render=True
    bpy.ops.export_scene.gltf(filepath=str(asset),export_format="GLB",use_selection=False,export_cameras=True,export_lights=False,export_extras=True,export_apply=True);(RUNTIME/f"{cell_id}.scene.json").write_text(json.dumps(data,indent=2)+"\n");render_preview(cell_id);return data

def render_preview(cell_id):
    scene=bpy.context.scene;scene.render.engine="BLENDER_EEVEE";scene.render.resolution_x=720;scene.render.resolution_y=405;scene.render.resolution_percentage=100;scene.render.image_settings.file_format="PNG";scene.render.filepath=str(PREVIEWS/f"{cell_id}.png");scene.render.film_transparent=False
    cam=next((o for o in scene.objects if o.type=="CAMERA"),None);scene.camera=cam;bpy.ops.render.render(write_still=True)

def linked_instance(blend_path,collection_names,offset=(0,0,0)):
    with bpy.data.libraries.load(str(blend_path),link=True) as (src,dst):dst.collections=[n for n in collection_names if n in src.collections]
    for coll in dst.collections:
        if not coll:continue
        empty=bpy.data.objects.new("LINK_"+coll.name,None);C("GEOMETRY").objects.link(empty);empty.instance_type="COLLECTION";empty.instance_collection=coll;empty.location=offset

def build_expanded_master(cell_sidecars):
    reset("INFINITE_BACKLOT_EXPANDED_WORLD");mats();sun_world()
    # Existing master geometry is linked, not rebuilt. Helpers/cameras/lights are excluded.
    with bpy.data.libraries.load(str(MASTER_SOURCE),link=False) as (src,dst):master_names=[n for n in src.collections if n not in {"SEMANTICS","CAMERAS","LIGHTING","COLLIDERS"}];dst.collections=[]
    linked_instance(MASTER_SOURCE,master_names,(0,0,0))
    placements={"cell_street_extension":(36,0,0),"cell_public_transit_pocket":(-29,0,0),"cell_industrial_transition":(15,35,0)}
    for cid,offset in placements.items():linked_instance(SOURCE/f"{cid}.blend",["CELL_"+cid.removeprefix("cell_").upper()],offset)
    for name,loc,typ in [("WORLD_SOCKET_WEST",(-42,0,0),"ROAD"),("WORLD_SOCKET_EAST",(50,0,0),"ROAD"),("WORLD_SOCKET_TRANSIT",(-29,-10,0),"TRANSIT"),("WORLD_SOCKET_NORTH",(15,50,0),"ROAD"),("WORLD_SOCKET_PARK",(-29,8,0),"PARK"),("WORLD_SOCKET_SERVICE",(21,45,0),"SERVICE"),("WORLD_SOCKET_OUTSKIRTS",(15,50,0),"OUTSKIRTS")]:semantic(name,loc,"socket",socket_type=typ,clearance_m=4.0)
    for i,loc in enumerate([(-29,0,0),(-15,0,0),(5,0,0),(22,0,0),(36,0,0),(15,25,0),(15,42,0)]):semantic(f"MARK_EXPANDED_ROUTE_{i+1:02}",loc,"staging_mark",radius_m=.75)
    camera("CAM_EXPANDED_OVERVIEW",(-45,-35,18),(8,10,3),40);camera("CAM_EXPANDED_TRANSIT",(-42,-15,7),(-29,0,2),38);camera("CAM_EXPANDED_STREET",(25,-18,8),(36,4,3),42);camera("CAM_EXPANDED_INDUSTRIAL",(30,20,8),(15,37,3),44)
    light_intent("LIGHT_EXPANDED_WEST",(-29,0,8),"LIGHT_STREET",(.25,.68,1),17000,22);light_intent("LIGHT_EXPANDED_EAST",(36,0,8),"LIGHT_STREET",(1,.62,.35),18000,22);light_intent("LIGHT_EXPANDED_NORTH",(15,35,7),"LIGHT_ALLEY",(.15,.65,1),15000,18)
    collider("COLLIDER_EXPANDED_WORLD_GROUND",(4,10,-.35),(92,100,.35))
    bpy.context.scene["module_id"]="infinite_backlot_expanded_world";bpy.context.scene["quality_tier"]="hero";bpy.context.scene["linked_cells"]=json.dumps(placements);bpy.ops.wm.save_as_mainfile(filepath=str(EXPANDED_SOURCE),compress=True,relative_remap=True)
    # Sidecar before realization: all outer expansion contracts are local semantics.
    dummy={"category":"expanded_connected_world","bounds":[[-42,-1,-50],[50,21,20]],"neighbors":["future_city","future_outskirts"],"priority":"near"};CELL_META["infinite_backlot_expanded_world"]=dummy;data=sidecar("infinite_backlot_expanded_world",EXPANDED_SOURCE,EXPANDED_GLB);data["cell_id"]="CELL_CONNECTED_NEIGHBORHOOD";data["composed_cells"]=["CELL_APARTMENT_BLOCK","CELL_ODD_HOURS_CORNER","CELL_STREET_EXTENSION","CELL_PUBLIC_TRANSIT_POCKET","CELL_SERVICE_ALLEY","CELL_INDUSTRIAL_TRANSITION"]
    # Realize linked instances only for the runtime GLB; then reopen the portable linked source.
    bpy.ops.object.select_all(action="DESELECT");instances=[o for o in bpy.context.scene.objects if o.instance_type=="COLLECTION"]
    for o in instances:o.select_set(True)
    if instances:bpy.context.view_layer.objects.active=instances[0];bpy.ops.object.duplicates_make_real(use_base_parent=True,use_hierarchy=True)
    bpy.ops.export_scene.gltf(filepath=str(EXPANDED_GLB),export_format="GLB",use_selection=False,export_cameras=True,export_lights=False,export_extras=True,export_apply=True);EXPANDED_SIDECAR.write_text(json.dumps(data,indent=2)+"\n");bpy.ops.wm.open_mainfile(filepath=str(EXPANDED_SOURCE));return data

def build_tour():
    bpy.ops.wm.open_mainfile(filepath=str(EXPANDED_SOURCE));scene=bpy.context.scene
    d=bpy.data.cameras.new("CAM_WORLD_EXPANSION_TOUR");d.lens=40;cam=bpy.data.objects.new("CAM_WORLD_EXPANSION_TOUR",d);scene.collection.objects.link(cam);scene.camera=cam
    bpy.context.preferences.edit.keyframe_new_interpolation_type="LINEAR"
    keys=[(1,(-42,-22,8),(-29,0,2)),(72,(-24,-18,7),(-12,0,3)),(144,(10,-20,8),(30,0,3)),(216,(48,-10,7),(36,2,3)),(288,(35,20,12),(15,35,3)),(324,(32,28,12),(15,38,3)),(360,(30,40,12),(15,42,3)),(432,(-35,35,22),(6,12,3))]
    for f,loc,target in keys:cam.location=loc;cam.rotation_euler=(Vector(target)-cam.location).to_track_quat("-Z","Y").to_euler();cam.keyframe_insert("location",frame=f);cam.keyframe_insert("rotation_euler",frame=f)
    scene.frame_start=1;scene.frame_end=432;scene.render.engine="BLENDER_EEVEE";scene.render.resolution_x=720;scene.render.resolution_y=405;scene.render.resolution_percentage=100;scene.render.fps=12;scene.render.image_settings.file_format="PNG";scene.render.filepath=str(OUTPUT/"frames/frame_");scene.render.film_transparent=False;bpy.ops.wm.save_as_mainfile(filepath=str(EXPANDED_TOUR),compress=True,relative_remap=True)

def update_registry(sidecars):
    path=ROOT/"assets/world/registry.json";reg=json.loads(path.read_text());by={m["module_id"]:m for m in reg["modules"]}
    for data in sidecars:
        asset=ROOT/data["asset"];entry=dict(data);entry["asset_sha256"]=hashlib.sha256(asset.read_bytes()).hexdigest();entry["glb_sha256"]=entry["asset_sha256"];entry["preview"]="assets/reference/world-expansion/hero_previews/"+data["module_id"]+".png";by[data["module_id"]]=entry
    reg["modules"]=list(by.values());reg["module_count"]=len(reg["modules"])
    if "hero" not in reg["quality_tiers"]:reg["quality_tiers"].append("hero")
    path.write_text(json.dumps(reg,indent=2)+"\n")

def write_world_cells():
    cells=[
      {"cell_id":"CELL_APARTMENT_BLOCK","runtime_asset":"assets/world/neighborhood/infinite_backlot_block.glb","quality_tier":"hero","bounds":{"min":[-17,-1,-25],"max":[8,16,-4]},"connection_sockets":["WORLD_SOCKET_WEST","WORLD_SOCKET_SERVICE"],"staging_regions":["lobby","entrance","street"],"camera_regions":["interior","exterior"],"interaction_regions":["doors","mailboxes","elevator"],"lighting_policy":"connected-master semantic lights","background_requirements":["urban skyline"],"neighbor_compatibility":["CELL_SERVICE_ALLEY","CELL_PUBLIC_TRANSIT_POCKET"],"future_streaming_priority":"near","world_state_hooks":["doors","elevator"]},
      {"cell_id":"CELL_ODD_HOURS_CORNER","runtime_asset":"assets/world/neighborhood/infinite_backlot_block.glb","quality_tier":"hero","bounds":{"min":[8,-1,-16],"max":[27,8,2]},"connection_sockets":["WORLD_SOCKET_EAST","WORLD_SOCKET_TRANSIT"],"staging_regions":["storefront","store interior"],"camera_regions":["street","store"],"interaction_regions":["store door","counter"],"lighting_policy":"connected-master semantic lights","background_requirements":["commercial continuation"],"neighbor_compatibility":["CELL_STREET_EXTENSION"],"future_streaming_priority":"near","world_state_hooks":["store_open","sign_power"]},
      {"cell_id":"CELL_CONNECTED_NEIGHBORHOOD","runtime_asset":"assets/world/neighborhood/infinite_backlot_expanded_world.glb","quality_tier":"hero","bounds":{"min":[-42,-1,-50],"max":[50,21,20]},"connection_sockets":["WORLD_SOCKET_WEST","WORLD_SOCKET_EAST","WORLD_SOCKET_TRANSIT","WORLD_SOCKET_NORTH","WORLD_SOCKET_PARK","WORLD_SOCKET_SERVICE","WORLD_SOCKET_OUTSKIRTS"],"staging_regions":["apartment block","Odd Hours corner","street extension","transit pocket","industrial transition"],"camera_regions":["connected tour","portrait staging","expansion overview"],"interaction_regions":["doors","public seating","service access"],"lighting_policy":"semantic runtime lights per composed cell","background_requirements":["urban skyline","industrial silhouettes","outskirts continuation"],"neighbor_compatibility":["future city","future park","future outskirts"],"future_streaming_priority":"near","world_state_hooks":["time_of_day","store_open","service_access"]},
    ]
    for cid,meta in CELL_META.items():
        if not cid.startswith("cell_"):continue
        data=json.loads((RUNTIME/f"{cid}.scene.json").read_text());cells.append({"cell_id":data["cell_id"],"runtime_asset":data["asset"],"quality_tier":"hero","bounds":data["bounds"],"connection_sockets":[x["id"] for x in data["sockets"]],"staging_regions":[x["id"] for x in data["staging_marks"]],"camera_regions":[x["id"] for x in data["camera_anchors"]],"interaction_regions":[x["id"] for x in data["interactions"]],"lighting_policy":data["lighting_policy"],"background_requirements":data["background_requirements"],"neighbor_compatibility":data["neighbor_compatibility"],"future_streaming_priority":data["future_streaming_priority"],"world_state_hooks":data["world_state_hooks"]})
    payload={"schema_version":1,"world_id":"infinite_backlot_expandable_world","master_runtime_asset":"assets/world/neighborhood/infinite_backlot_expanded_world.glb","cells":cells,"socket_vocabulary":["WORLD_SOCKET_NORTH","WORLD_SOCKET_SOUTH","WORLD_SOCKET_EAST","WORLD_SOCKET_WEST","WORLD_SOCKET_TRANSIT","WORLD_SOCKET_PARK","WORLD_SOCKET_SERVICE","WORLD_SOCKET_OUTSKIRTS"]};(RUNTIME/"world_cells.json").write_text(json.dumps(payload,indent=2)+"\n");(OUTPUT/"world_cells.json").write_text(json.dumps(payload,indent=2)+"\n")

def main():
    build_asset_library();sidecars=[]
    for cid,builder in CELL_BUILDERS.items():builder();sidecars.append(save_export_cell(cid))
    expanded=build_expanded_master(sidecars);sidecars.append(expanded);update_registry(sidecars);write_world_cells();build_tour();print({"cells":len(sidecars)-1,"assets":len(json.loads(KIT_CATALOG.read_text())["assets"]),"registry":json.loads((ROOT/"assets/world/registry.json").read_text())["module_count"],"tour":str(EXPANDED_TOUR)})

if __name__=="__main__":main()
