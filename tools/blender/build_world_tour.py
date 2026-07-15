"""Assemble and render the low-cost reusable-world tour.

Build in Blender MCP with BUILD_ONLY=True, then render in background with
BUILD_ONLY=False (the default).
"""
from __future__ import annotations

import json
import math
import os
from pathlib import Path

import bpy
from mathutils import Vector

ROOT=Path(r"C:/Projects/bevy-infinite")
LAYOUT_PATH=ROOT/"data/world/demo_world_seed_424242.json"
REGISTRY_PATH=ROOT/"assets/world/registry.json"
BLEND_PATH=ROOT/"assets/source/blender/world/demo_world_tour.blend"
OUTPUT_PATH=ROOT/"output/world-tour/world_tour.mp4"
FRAME_DIR=ROOT/"output/world-tour/frames"
BUILD_ONLY=bool(globals().get("BUILD_ONLY",False))

SHOWCASE_ROLES={
    "ground_lobby":(-17,0,0),"ground_elevator":(-10,0,0),
    "floor_one_hall":(-17,8,0),"floor_two_junction":(-10,8,0),
    "building_exterior":(-1,0,0),"street_intersection":(10,0,0),
    "alley":(20,0,0),"hero_store": (29,0,0),"pocket_park": (39,0,0),
}


def reset():
    bpy.ops.object.select_all(action="SELECT"); bpy.ops.object.delete(use_global=False)
    for blockset in (bpy.data.meshes,bpy.data.curves,bpy.data.materials,bpy.data.cameras,bpy.data.lights):
        for block in list(blockset):
            if block.users==0: blockset.remove(block)


def label(text,loc):
    curve=bpy.data.curves.new(f"LABEL_{text}","FONT"); curve.body=text.replace("_"," ").upper(); curve.align_x="CENTER"; curve.size=.62; curve.extrude=.018
    obj=bpy.data.objects.new(f"LABEL_{text}",curve); bpy.context.scene.collection.objects.link(obj); obj.location=loc; obj.rotation_euler=(math.radians(68),0,0)
    mat=bpy.data.materials.get("MAT_Label")
    if not mat:
        mat=bpy.data.materials.new("MAT_Label"); mat.diffuse_color=(1,.55,.08,1); mat.use_nodes=True; bsdf=mat.node_tree.nodes.get("Principled BSDF"); bsdf.inputs["Base Color"].default_value=mat.diffuse_color; bsdf.inputs["Emission Color"].default_value=mat.diffuse_color; bsdf.inputs["Emission Strength"].default_value=1.8
    obj.data.materials.append(mat)


def import_module(role,module,display):
    before=set(bpy.context.scene.objects)
    bpy.ops.import_scene.gltf(filepath=str(ROOT/module["asset"]))
    added=[o for o in bpy.context.scene.objects if o not in before]
    root=bpy.data.objects.new(f"INSTANCE_{role}",None); bpy.context.scene.collection.objects.link(root); root.location=display
    for o in added:
        if o.type in {"CAMERA","LIGHT"} or o.get("semantic_kind") in {"socket","staging_mark","camera_anchor","interaction","collider","cutaway"}:
            o.hide_render=True
        o.parent=root
    label(role,(display[0],display[1]-4.5,0.05))


def build():
    reset(); layout=json.loads(LAYOUT_PATH.read_text()); registry=json.loads(REGISTRY_PATH.read_text()); modules={m["module_id"]:m for m in registry["modules"]}
    for entry in layout["instances"]:
        role=entry["role"]
        if role in SHOWCASE_ROLES: import_module(role,modules[entry["module_id"]],SHOWCASE_ROLES[role])
    scene=bpy.context.scene; scene.unit_settings.system="METRIC"; scene.render.engine="BLENDER_EEVEE"; scene.world.color=(.012,.02,.035)
    # Exhibition floor and visual separators.
    mat=bpy.data.materials.new("MAT_TourFloor"); mat.diffuse_color=(.025,.035,.05,1); mat.use_nodes=True
    bpy.ops.mesh.primitive_plane_add(size=120,location=(10,2,-.18)); floor=bpy.context.object; floor.name="SET_TourFloor"; floor.data.materials.append(mat)
    sun_data=bpy.data.lights.new("TourSun","SUN"); sun_data.energy=1.8; sun_data.angle=math.radians(24); sun=bpy.data.objects.new("TourSun",sun_data); scene.collection.objects.link(sun); sun.rotation_euler=(math.radians(32),math.radians(-18),math.radians(28))
    for name,loc,energy,size in (("TourKey",(-10,-8,14),1800,8),("TourFill",(25,-4,12),1400,10)):
        data=bpy.data.lights.new(name,"AREA"); data.energy=energy; data.size=size; obj=bpy.data.objects.new(name,data); scene.collection.objects.link(obj); obj.location=loc; obj.rotation_euler=(Vector((10,2,1))-obj.location).to_track_quat("-Z","Y").to_euler()
    cam_data=bpy.data.cameras.new("CAM_WorldTour"); cam=bpy.data.objects.new("CAM_WorldTour",cam_data); scene.collection.objects.link(cam); scene.camera=cam; cam_data.lens=34
    keys=[
        (1,(-23,-13,8),(-14,1,1.3)),(35,(-16,-11,6),(-13,2,1.4)),(70,(-8,-10,7),(-6,2,2.6)),
        (105,(-3,-11,7),(1,1,3.0)),(140,(7,-12,7),(11,1,1.3)),(175,(17,-10,6),(20,1,1.4)),
        (215,(27,-11,6),(29,1,1.6)),(255,(38,-10,7),(39,1,1.8)),(288,(46,-8,10),(32,2,1.8)),
    ]
    for frame,pos,target in keys:
        cam.location=pos; cam.rotation_euler=(Vector(target)-cam.location).to_track_quat("-Z","Y").to_euler(); cam.keyframe_insert("location",frame=frame); cam.keyframe_insert("rotation_euler",frame=frame)
    scene.frame_start=1; scene.frame_end=288; scene.render.fps=12; scene.render.resolution_x=640; scene.render.resolution_y=360; scene.render.resolution_percentage=100
    scene.render.image_settings.file_format="PNG"; FRAME_DIR.mkdir(parents=True,exist_ok=True); scene.render.filepath=str(FRAME_DIR/"frame_")
    BLEND_PATH.parent.mkdir(parents=True,exist_ok=True); OUTPUT_PATH.parent.mkdir(parents=True,exist_ok=True); bpy.ops.wm.save_as_mainfile(filepath=str(BLEND_PATH))
    print(f"WORLD_TOUR_BUILT roles={len(SHOWCASE_ROLES)} blend={BLEND_PATH}")


build()
if not BUILD_ONLY:
    bpy.ops.render.render(animation=True)
    print(f"WORLD_TOUR_FRAMES_RENDERED {FRAME_DIR}")
