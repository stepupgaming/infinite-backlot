"""One-load Kimodo batch generator for the Infinite Backlot motion lab.

Unlike the legacy subprocess batch wrapper, this keeps the Kimodo/SOMA model and
text encoder resident while generating many independent semantic clips.
"""
from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path

import numpy as np
import torch

from kimodo import load_model
from kimodo.constraints import load_constraints_lst
from kimodo.exports.bvh import save_motion_bvh
from kimodo.exports.motion_io import save_kimodo_npz
from kimodo.skeleton import SOMASkeleton30, global_rots_to_local_rots
from kimodo.tools import seed_everything


def emit(phase:str,**fields):
    print(json.dumps({"phase":phase,**fields},separators=(",",":")),flush=True)


def write_constraints(request:dict)->Path|None:
    waypoints=sorted(request.get("root_waypoints") or [],key=lambda item:int(item["frame"]))
    if not waypoints: return None
    if len({int(item["frame"]) for item in waypoints})<2: raise ValueError("root waypoints need two distinct frames")
    path=Path(request["output_stem"]).with_suffix(".constraints.json"); path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(json.dumps([{"type":"root2d","frame_indices":[int(item["frame"]) for item in waypoints],"smooth_root_2d":[[float(item["x"]),float(item["z"])] for item in waypoints]}],indent=2),encoding="utf-8")
    return path


def main()->int:
    parser=argparse.ArgumentParser(); parser.add_argument("--checkpoint",required=True); parser.add_argument("--requests",required=True); parser.add_argument("--responses",required=True); parser.add_argument("--diffusion-steps",type=int,default=12); args=parser.parse_args()
    requests=json.loads(Path(args.requests).read_text(encoding="utf-8")); checkpoint=Path(args.checkpoint).resolve()
    if not checkpoint.is_dir(): raise FileNotFoundError(checkpoint)
    os.environ["CHECKPOINT_DIR"]=str(checkpoint.parent); os.environ.setdefault("LOCAL_CACHE","true"); os.environ.setdefault("TEXT_ENCODER_MODE","local"); os.environ.setdefault("HUGGINGFACE_CACHE_DIR",r"F:\Models\huggingface\hub")
    device="cuda:0" if torch.cuda.is_available() else "cpu"; emit("kimodo.model_load.started",device=device,model=checkpoint.name)
    loaded=time.perf_counter(); model,resolved=load_model(checkpoint.name,device=device,default_family="Kimodo",return_resolved_name=True); emit("kimodo.model_load.completed",elapsed_ms=round((time.perf_counter()-loaded)*1000),resolved_model=resolved)
    skeleton=model.skeleton
    bvh_skeleton=skeleton.somaskel77.to(device) if isinstance(skeleton,SOMASkeleton30) else skeleton
    responses=[]
    for index,request in enumerate(requests):
        started=time.perf_counter(); stem=Path(request["output_stem"]); stem.parent.mkdir(parents=True,exist_ok=True); constraints_path=write_constraints(request)
        constraints=load_constraints_lst(str(constraints_path),skeleton) if constraints_path else []
        frames=max(2,round(float(request["duration"])*float(model.fps))); seed_everything(int(request.get("seed",0)))
        emit("kimodo.inference.started",index=index,semantic=request["semantic"],frames=frames)
        output=model([request["prompt"]],[frames],constraint_lst=constraints,num_denoising_steps=args.diffusion_steps,num_samples=1,multi_prompt=True,num_transition_frames=10,post_processing=True,return_numpy=True)
        single={key:(value[0] if hasattr(value,"shape") and len(value.shape)>0 and value.shape[0]==1 else value) for key,value in output.items()}
        npz=stem.with_suffix(".npz"); bvh=stem.with_suffix(".bvh"); sidecar=stem.with_suffix(".motion.json")
        save_kimodo_npz(str(npz),single)
        joints=torch.from_numpy(output["posed_joints"][0]).to(device); rots=torch.from_numpy(output["global_rot_mats"][0]).to(device); local=global_rots_to_local_rots(rots,bvh_skeleton); root=joints[:,bvh_skeleton.root_idx,:]
        save_motion_bvh(str(bvh),local,root,skeleton=bvh_skeleton,fps=model.fps,standard_tpose=True)
        root_positions=np.asarray(single["root_positions"],dtype=np.float32); contacts=np.asarray(single["foot_contacts"],dtype=np.bool_); posed=np.asarray(single["posed_joints"],dtype=np.float32); contact_indices=[69,70,71,74,75,76]
        sidecar.write_text(json.dumps({"schema_version":1,"sample_rate":float(model.fps),"root_positions":root_positions.tolist(),"foot_contacts":contacts.tolist(),"foot_positions":posed[:,contact_indices,:].tolist(),"contact_channels":[f"contact_{i}" for i in range(contacts.shape[1])]},separators=(",",":")),encoding="utf-8")
        response={"index":index,"semantic":request["semantic"],"npz":str(npz.resolve()),"bvh":str(bvh.resolve()),"motion_sidecar":str(sidecar.resolve()),"constraints":str(constraints_path.resolve()) if constraints_path else None,"elapsed_ms":round((time.perf_counter()-started)*1000),"success":True}; responses.append(response); emit("kimodo.motion_export.completed",**response)
        del output,single,joints,rots,local,root
    response_path=Path(args.responses); response_path.parent.mkdir(parents=True,exist_ok=True); response_path.write_text(json.dumps(responses,indent=2),encoding="utf-8"); emit("kimodo.complete",response_count=len(responses)); return 0


if __name__=="__main__": raise SystemExit(main())
