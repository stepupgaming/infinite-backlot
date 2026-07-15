"""Render the collision-safe Kimodo navigation proof and control reel.

Uses the generated SOMA pose/rotation tensors for the performer, the authored
navigation contract for space, and the selected candidate metrics for evidence.
"""
from __future__ import annotations
import importlib.util
import json
import math
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw, ImageFont

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/"runtimes/kimodo"))
from kimodo.skeleton import SOMASkeleton77

NAV_OUT=ROOT/"output/navigation-kimodo-proof"
SHOW_OUT=ROOT/"output/kimodo-control-showcase"
NAV_FILE=NAV_OUT/"candidates/selected.npz"
PANEL_FILE=SHOW_OUT/"panel_candidates/selected.npz"
WIDTH,HEIGHT,FPS=960,540,15
FONT=ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf",16)
BOLD=ImageFont.truetype(r"C:\Windows\Fonts\segoeuib.ttf",20)
SMALL=ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf",12)


def load_skin_class():
 path=ROOT/"runtimes/kimodo/kimodo/viz/soma_skin.py";spec=importlib.util.spec_from_file_location("backlot_soma_skin",path);mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod);return mod.SOMASkin

def load_motion(path):
 d=np.load(path);j=np.asarray(d["posed_joints"],np.float32);r=np.asarray(d["global_rot_mats"],np.float32);root=np.asarray(d["root_positions"],np.float32);contacts=np.asarray(d["foot_contacts"],bool)
 for name,value in [("j",j),("r",r),("root",root),("contacts",contacts)]:
  if value.ndim in {3,5} and value.shape[0]==1:locals()[name]=value[0]
 if j.ndim==4:j=j[0]
 if r.ndim==5:r=r[0]
 if root.ndim==3:root=root[0]
 if contacts.ndim==3:contacts=contacts[0]
 skin=load_skin_class()(SOMASkeleton77())
 with torch.inference_mode():verts=skin.skin(torch.from_numpy(r),torch.from_numpy(j),rot_is_global=True).cpu().numpy().astype(np.float32)
 return {"joints":j,"rots":r,"root":root,"contacts":contacts,"vertices":verts,"faces":skin.faces.cpu().numpy().astype(np.int32)}

def interp_target(request,frames):
 s=request["dense_root_path"];times=np.asarray([x["time"] for x in s],np.float32);vals=np.asarray([[x["position"][0],x["position"][2]] for x in s],np.float32);q=np.linspace(0,request["duration"],frames,dtype=np.float32);return np.stack([np.interp(q,times,vals[:,a]) for a in range(2)],axis=1)

def correct_root(motion,request,max_corridor=.18):
 generated=motion["root"][:,[0,2]];target=interp_target(request,len(generated));delta=target-generated;distance=np.linalg.norm(delta,axis=1);factor=np.where(distance>max_corridor,1-max_corridor/np.maximum(distance,1e-8),0);corrected=generated+delta*factor[:,None];return target,generated,corrected

def metrics(a,b):
 d=np.linalg.norm(a-b,axis=1);return {"max_m":float(d.max(initial=0)),"mean_m":float(d.mean())}

def map_point(x,z):
 return (int(38+(x+5.5)/29*515),int(500-(z+15.5)/13*430))

def background(world,debug):
 im=Image.new("RGB",(WIDTH,HEIGHT),(8,13,23));d=ImageDraw.Draw(im,"RGBA");d.rounded_rectangle((18,18,570,520),14,fill=(15,24,38,255),outline=(57,78,99,255),width=2);d.rounded_rectangle((586,18,942,520),14,fill=(13,20,32,255),outline=(57,78,99,255),width=2)
 for region in world["regions"]:
  pts=[map_point(x,z) for x,z in region["polygon"]];d.polygon(pts,fill=(20,90,94,55) if debug else (30,52,63,100),outline=(53,220,220,150) if debug else (65,82,95,180))
 for c in world["colliders"]:
  x,_,z=c["center"];hx,_,hz=c["half_extents"];a=map_point(x-hx,z-hz);b=map_point(x+hx,z+hz);d.rectangle((min(a[0],b[0]),min(a[1],b[1]),max(a[0],b[0]),max(a[1],b[1])),fill=(225,55,75,120) if debug else (91,51,58,220),outline=(255,105,120,180))
 if debug:
  for p in world["portals"]:
   x,y=map_point(p["position"][0],p["position"][2]);d.line((x-8,y,x+8,y),fill=(255,202,73,255),width=4)
 d.text((34,30),"CONNECTED WORLD / TOP-DOWN",font=BOLD,fill=(235,245,255,255));d.text((606,30),"SOMA PERFORMANCE / GENERATED BODY",font=BOLD,fill=(235,245,255,255));return im

def line(draw,points,color,width=2):
 if len(points)>1:draw.line([map_point(float(p[0]),float(p[1])) for p in points],fill=color,width=width,joint="curve")

def draw_mesh(im,vertices,root,faces,color):
 d=ImageDraw.Draw(im,"RGBA");v=vertices.copy();v[:,0]-=root[0];v[:,2]-=root[2];sx=v[:,0]+.32*v[:,2];sy=v[:,1]-.08*v[:,2];pts=np.stack([762+sx*178,455-sy*178],axis=1);depth=v[:,2]-.2*v[:,0];chosen=faces[::12];order=np.argsort(depth[chosen].mean(axis=1))
 for idx in order:
  tri=[tuple(pts[i]) for i in chosen[idx]]
  if max(p[0] for p in tri)<590 or min(p[0] for p in tri)>940:continue
  d.polygon(tri,fill=color)
 d.ellipse((pts[:,0].mean()-70,472,pts[:,0].mean()+70,490),fill=(0,0,0,90))

def current_prompt(request,t):
 for p in request["prompt_sequence"]:
  if p["start"]<=t<=p["end"]+1e-4:return p["text"]
 return request["prompt_sequence"][-1]["text"]

def frame_image(world,route,request,motion,target,generated,corrected,frame,debug,label=None):
 im=background(world,debug);d=ImageDraw.Draw(im,"RGBA")
 if debug:
  line(d,[(p[0],p[2]) for p in route["raw_path"]],(255,160,40,220),2);line(d,[(p[0],p[2]) for p in route["smoothed_path"]],(50,220,255,220),3);line(d,target,(120,255,125,190),2);line(d,generated,(255,80,210,180),2);line(d,corrected,(255,255,255,230),2)
 else:line(d,corrected,(70,205,226,110),2)
 x,z=corrected[frame];cx,cy=map_point(x,z);radius=int(.34/29*515);d.ellipse((cx-radius,cy-radius,cx+radius,cy+radius),fill=(240,245,255,230),outline=(40,220,255,255),width=2)
 if debug:
  for c in request.get("end_effector_constraints",[]):
   tx,ty=map_point(c["position"][0],c["position"][2]);d.ellipse((tx-6,ty-6,tx+6,ty+6),outline=(255,220,65,255),width=2)
  foot=motion["joints"][frame]
  for name in ["LeftFoot","RightFoot"]:
   idx=SOMASkeleton77().bone_index[name];fx,fy=map_point(foot[idx,0],foot[idx,2]);d.ellipse((fx-3,fy-3,fx+3,fy+3),fill=(170,90,255,255))
 corrected_vertices=motion["vertices"][frame].copy();shift=corrected[frame]-generated[frame];corrected_vertices[:,0]+=shift[0];corrected_vertices[:,2]+=shift[1];root3=motion["root"][frame].copy();root3[[0,2]]=corrected[frame];draw_mesh(im,corrected_vertices,root3,motion["faces"],(38,215,215,235) if not debug else (210,225,235,230))
 t=frame/30;prompt=current_prompt(request,t);d.rounded_rectangle((600,70,928,129),8,fill=(0,0,0,150));d.text((612,80),prompt[:69],font=SMALL,fill=(220,232,242,255));d.text((612,101),prompt[69:138],font=SMALL,fill=(220,232,242,255));d.text((36,492),f"t={t:05.2f}s  portal-safe  capsule r=0.34m",font=SMALL,fill=(210,226,238,255))
 if label:
  d.rounded_rectangle((600,442,928,500),8,fill=(0,0,0,180),outline=(49,220,230,150));d.text((614,455),label,font=BOLD,fill=(255,255,255,255))
 if debug:
  d.text((603,145),"orange raw / cyan smooth / green requested",font=SMALL,fill=(218,230,242,255));d.text((603,162),"magenta generated / white corrected runtime",font=SMALL,fill=(218,230,242,255));d.text((603,179),"yellow contacts / purple feet",font=SMALL,fill=(218,230,242,255))
 return im

def encode(frame_dir,out):
 subprocess.run(["ffmpeg","-y","-hide_banner","-loglevel","error","-framerate",str(FPS),"-i",str(frame_dir/"frame_%05d.png"),"-c:v","libx264","-preset","medium","-crf","18","-pix_fmt","yuv420p","-movflags","+faststart",str(out)],check=True)

def render_navigation(world,route,request,motion,target,generated,corrected):
 with tempfile.TemporaryDirectory(prefix="backlot_nav_proof_") as tmp:
  base=Path(tmp);clean=base/"clean";debug=base/"debug";clean.mkdir();debug.mkdir()
  out_index=0
  for frame in range(0,len(generated),2):
   frame_image(world,route,request,motion,target,generated,corrected,frame,False).save(clean/f"frame_{out_index:05d}.png")
   frame_image(world,route,request,motion,target,generated,corrected,frame,True).save(debug/f"frame_{out_index:05d}.png")
   out_index+=1
  encode(clean,NAV_OUT/"navigation_clean.mp4");encode(debug,NAV_OUT/"navigation_debug.mp4")

def render_controls(world,route,nav_request,nav,panel_request,panel,nav_roots,panel_roots,selected_metrics):
 segments=[
  ("curved_root_path","Curved root path",nav,nav_request,nav_roots,0.0,2.0),("root_waypoints","Sparse root waypoints",nav,nav_request,nav_roots,2.0,4.0),("prompt_sequence","Prompt sequence transition",nav,nav_request,nav_roots,5.8,7.2),("full_body_keyframe","Full-body keyframe in-betweening",nav,nav_request,nav_roots,7.2,8.5),
  ("hand_position","Hand position constraint",nav,nav_request,nav_roots,15.7,16.0),("hand_rotation","Hand rotation constraint",nav,nav_request,nav_roots,16.0,16.4),("foot_constraint","Foot position + rotation",nav,nav_request,nav_roots,17.6,17.9),("mixed_constraints","Mixed root + hand + foot",nav,nav_request,nav_roots,17.9,18.0),
  ("panel_interaction","Smart panel press",panel,panel_request,panel_roots,3.35,4.45),("door_interaction","Smart door interaction",nav,nav_request,nav_roots,14.6,16.5),("pickup_interaction","Smart counter pickup",nav,nav_request,nav_roots,16.8,18.0),
 ]
 with tempfile.TemporaryDirectory(prefix="backlot_kimodo_controls_") as tmp:
  base=Path(tmp);clean=base/"clean";debug=base/"debug";clean.mkdir();debug.mkdir();frame_no=0;index=[];reel_t=0.0
  for sid,label,motion,request,roots,start,end in segments:
   target,generated,corrected=roots;start_reel=reel_t
   for frame in range(round(start*30),min(len(generated),round(end*30)),2):
    frame_image(world,route,request,motion,target,generated,corrected,frame,False,label).save(clean/f"frame_{frame_no:05d}.png");frame_image(world,route,request,motion,target,generated,corrected,frame,True,label).save(debug/f"frame_{frame_no:05d}.png");frame_no+=1;reel_t+=1/FPS
   index.append({"id":sid,"label":label,"reel_start":round(start_reel,3),"reel_end":round(reel_t,3),"source_request":request["request_id"],"source_start":start,"source_end":end})
  # Candidate-selection end card is derived from the real scored candidates.
  for _ in range(round(1.8*FPS)):
   im=Image.new("RGB",(WIDTH,HEIGHT),(7,13,24));d=ImageDraw.Draw(im);d.text((70,80),"BOUNDED CANDIDATE SELECTION",font=BOLD,fill=(235,245,255));d.text((70,135),f"selected seed: {selected_metrics['seed']}",font=BOLD,fill=(70,225,220));d.text((70,180),f"score: {selected_metrics['score']:.4f} / valid: {selected_metrics['valid']}",font=BOLD,fill=(255,210,90));d.text((70,225),f"root max error: {selected_metrics['metrics']['root_path_deviation']:.3f} m",font=FONT,fill=(220,230,240));d.text((70,255),f"hand target error: {selected_metrics['metrics']['hand_target_error']:.3f} m",font=FONT,fill=(220,230,240));d.text((70,285),f"obstacle intersections: {selected_metrics['metrics']['body_obstacle_intersections']}",font=FONT,fill=(220,230,240));im.save(clean/f"frame_{frame_no:05d}.png");im.save(debug/f"frame_{frame_no:05d}.png");frame_no+=1;reel_t+=1/FPS
  index.append({"id":"candidate_selection","label":"Candidate scoring and selection","reel_start":round(reel_t-1.8,3),"reel_end":round(reel_t,3),"selected_seed":selected_metrics["seed"]})
  encode(clean,SHOW_OUT/"kimodo_controls.mp4");encode(debug,SHOW_OUT/"kimodo_controls_debug.mp4");(SHOW_OUT/"index.json").write_text(json.dumps({"schema_version":1,"performer":"SOMA77","segments":index},indent=2)+"\n")

def make_sheets():
 subprocess.run(["ffmpeg","-y","-hide_banner","-loglevel","error","-i",str(NAV_OUT/"navigation_debug.mp4"),"-vf","fps=1/3,scale=320:180,tile=3x2","-frames:v","1",str(NAV_OUT/"contact_sheet.png")],check=True)
 subprocess.run(["ffmpeg","-y","-hide_banner","-loglevel","error","-i",str(SHOW_OUT/"kimodo_controls_debug.mp4"),"-vf","fps=1,scale=240:135,tile=4x3","-frames:v","1",str(SHOW_OUT/"contact_sheet.png")],check=True)

def normalize_batch_outputs(directory):
 manifest_path=directory/"kimodo_response_manifest.json";score_path=directory/"candidate_scores.json";manifest=json.loads(manifest_path.read_text());scores=json.loads(score_path.read_text());candidates=scores if isinstance(scores,list) else scores["candidates"];selected=manifest.get("selected_candidate")
 if selected is None:raise RuntimeError(f"no valid Kimodo candidate in {manifest_path}")
 for candidate in candidates:candidate["selected"]=candidate["index"]==selected["index"]
 for key,filename in [("npz","selected.npz"),("bvh","selected.bvh"),("motion_sidecar","selected.motion.json")]:
  target=directory/filename
  if not target.exists():shutil.copy2(selected[key],target)
 payload={"schema_version":1,"selected_index":selected["index"],"candidates":candidates};score_path.write_text(json.dumps(payload,indent=2)+"\n");return payload,selected


def main():
 nav_scores,nav_selected=normalize_batch_outputs(NAV_OUT/"candidates");panel_scores,panel_selected=normalize_batch_outputs(SHOW_OUT/"panel_candidates")
 for p in [NAV_FILE,PANEL_FILE]:
  if not p.exists():raise FileNotFoundError(p)
 world=json.loads((ROOT/"assets/world/navigation/connected_navigation.json").read_text());route=json.loads((NAV_OUT/"resolved_route.json").read_text());nav_req=json.loads((NAV_OUT/"kimodo_request.json").read_text());requests=json.loads((SHOW_OUT/"kimodo_requests.json").read_text());panel_req=next(x for x in requests if x["request_id"]=="SOMA_PANEL_PRESS_MIXED_CONSTRAINTS")
 nav=load_motion(NAV_FILE);panel=load_motion(PANEL_FILE);nav_roots=(*correct_root(nav,nav_req),);panel_roots=(*correct_root(panel,panel_req),)
 nav_target,nav_generated,nav_corrected=nav_roots;correction=metrics(nav_generated,nav_corrected);request_error=metrics(nav_target,nav_generated);final_error=metrics(nav_target,nav_corrected)
 selected={"seed":nav_selected["seed"],**nav_selected["evaluation"]}
 contact_failures=int(selected["metrics"]["hand_target_error"]>.25 or selected["metrics"]["hand_orientation_error_deg"]>45)
 roots3=[]
 for i,p in enumerate(nav_corrected):roots3.append([float(p[0]),float(nav["root"][i,1]),float(p[1])])
 (NAV_OUT/"kimodo_root_paths.json").write_text(json.dumps({"requested_vs_generated":request_error,"generated_vs_corrected":correction,"requested_vs_final":final_error,"requested_root":[[float(x),0,float(z)] for x,z in nav_target],"generated_root":[[float(x),float(nav["root"][i,1]),float(z)] for i,(x,z) in enumerate(nav_generated)],"corrected_root":roots3,"final_runtime_root":roots3,"final_runtime_ground_root":[[point[0],0.0,point[2]] for point in roots3],"interaction_contact_failures":contact_failures},indent=2)+"\n")
 shutil.copy2(NAV_OUT/"candidates/kimodo_response_manifest.json",NAV_OUT/"kimodo_response_manifest.json");(SHOW_OUT/"candidate_scores.json").write_text(json.dumps({"navigation":nav_scores,"panel":panel_scores},indent=2)+"\n")
 render_navigation(world,route,nav_req,nav,nav_target,nav_generated,nav_corrected);render_controls(world,route,nav_req,nav,panel_req,panel,nav_roots,panel_roots,selected);make_sheets();print(json.dumps({"navigation_frames":len(nav_generated),"panel_frames":len(panel["root"]),"generated_root_max_error_m":request_error["max_m"],"correction_max_m":correction["max_m"],"final_root_max_error_m":final_error["max_m"],"contact_failures":contact_failures}))
if __name__=="__main__":main()
