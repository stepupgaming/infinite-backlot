"""Author the single economical connected-neighborhood tour.

The tour follows one continuous spatial path; it contains no module-showroom cuts.
Rendering is performed later in Blender background mode to a recoverable PNG sequence.
"""
from pathlib import Path
import math
import bpy
from mathutils import Vector

ROOT=Path(r"C:/Projects/bevy-infinite")
MASTER=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_block.blend"
TOUR=ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_block_tour.blend"
OUT=ROOT/"output/world-art-pass"
FRAMES=OUT/"frames"
PREVIEW=ROOT/"assets/reference/world-art-pass/master_neighborhood.png"
FRAMES.mkdir(parents=True,exist_ok=True);PREVIEW.parent.mkdir(parents=True,exist_ok=True)

bpy.ops.wm.open_mainfile(filepath=str(MASTER))
scene=bpy.context.scene
scene.render.engine="BLENDER_EEVEE"
scene.render.resolution_x=960;scene.render.resolution_y=540;scene.render.resolution_percentage=100
scene.render.image_settings.file_format="PNG";scene.render.film_transparent=False
scene.render.fps=12;scene.frame_start=1;scene.frame_end=480
scene.world.color=(.025,.035,.055)
for name in ("SEMANTICS","CAMERAS","COLLIDERS","CUTAWAYS"):
    coll=bpy.data.collections.get(name)
    if coll:coll.hide_render=True
# Open only the two tour-facing entry doors. Their runtime-controlled objects remain
# in the production master and GLB; this is presentation state, not an asset deletion.
for obj in scene.objects:
    if obj.get("kit_asset_id")=="entry_door" and (abs(obj.location.x)<2 or abs(obj.location.x-17)<2):
        obj.hide_render=True

# Warm late-afternoon exterior plus a broad soft fill. Practical emissive meshes and
# authored interior areas remain visible inside the locations.
sun_data=bpy.data.lights.new("TOUR_Sun","SUN");sun_data.energy=2.4;sun_data.angle=math.radians(28);sun_data.color=(1.0,.66,.42)
sun=bpy.data.objects.new("TOUR_Sun",sun_data);scene.collection.objects.link(sun);sun.rotation_euler=(math.radians(32),math.radians(-22),math.radians(25))
fill_data=bpy.data.lights.new("TOUR_SkyFill","AREA");fill_data.energy=1100;fill_data.shape="DISK";fill_data.size=22;fill_data.color=(.34,.52,1.0)
fill=bpy.data.objects.new("TOUR_SkyFill",fill_data);scene.collection.objects.link(fill);fill.location=(3,-2,18);fill.rotation_euler=(0,0,0)


def point_camera(name,loc,target,lens):
    data=bpy.data.cameras.new(name);data.lens=lens;data.sensor_width=36
    obj=bpy.data.objects.new(name,data);scene.collection.objects.link(obj);obj.location=loc;obj.rotation_euler=(Vector(target)-obj.location).to_track_quat("-Z","Y").to_euler();return obj

# Master overview reference, separate from the animated tour.
aerial=point_camera("CAM_MASTER_AERIAL_PREVIEW",(34,-28,21),(3,11,3.8),48);scene.camera=aerial;scene.render.resolution_x=1280;scene.render.resolution_y=720;scene.render.filepath=str(PREVIEW);bpy.ops.render.render(write_still=True)

cam=point_camera("CAM_WORLD_ART_TOUR",(18,-14,3.5),(4,6,3),38);scene.camera=cam
keys=[
(1,(18,-14,3.5),(4,6,3),38),
(60,(9,-8,2.5),(0,7,2.7),38),
(100,(3,2.6,1.75),(0,7,1.65),40),
(140,(0,8.2,1.72),(0,12.0,1.35),36),
(180,(0,11.0,1.72),(0,14.7,1.35),42),
(220,(0,8.2,1.72),(0,5.6,1.35),40),
(260,(-6.0,5.3,1.75),(-10,7.4,1.25),42),
(300,(-10,8.0,1.72),(-10,14.5,1.15),50),
(340,(-10,17.8,1.72),(-10,12.0,1.15),54),
(380,(-10,7.8,1.72),(-4,5.4,1.25),46),
(410,(2.0,4.8,1.85),(14,6.5,1.8),44),
(440,(14.0,5.2,1.75),(17,8.8,1.45),40),
(470,(17,7.8,1.72),(17,12.5,1.25),38),
(480,(14.3,9.0,1.68),(19.8,8.6,1.2),50),
]
for frame,loc,target,lens in keys:
    cam.location=loc;cam.rotation_euler=(Vector(target)-Vector(loc)).to_track_quat("-Z","Y").to_euler();cam.data.lens=lens
    cam.keyframe_insert("location",frame=frame);cam.keyframe_insert("rotation_euler",frame=frame);cam.data.keyframe_insert("lens",frame=frame)
# Linear interpolation prevents Bezier overshoot through the entrance, walls, and
# stocked aisles while still producing a continuous camera move.
for animated in (cam,cam.data):
    action=animated.animation_data.action if animated.animation_data else None
    if not action:continue
    for layer in action.layers:
        for strip in layer.strips:
            for bag in strip.channelbags:
                for fcurve in bag.fcurves:
                    for key in fcurve.keyframe_points:key.interpolation="LINEAR"
scene.render.resolution_x=960;scene.render.resolution_y=540;scene.render.filepath=str(FRAMES/"frame_")
scene["tour_kind"]="continuous_connected_neighborhood";scene["tour_route"]="street>exterior>entrance>lobby>elevator>entrance>sidewalk>alley>sidewalk>store";scene["render_helpers_hidden"]=True
bpy.ops.wm.save_as_mainfile(filepath=str(TOUR),compress=True);bpy.ops.file.make_paths_relative();bpy.ops.wm.save_as_mainfile(filepath=str(TOUR),compress=True)
print(f"WORLD_ART_TOUR_AUTHORED frames={scene.frame_start}-{scene.frame_end} fps={scene.render.fps} source={TOUR} preview={PREVIEW}")
