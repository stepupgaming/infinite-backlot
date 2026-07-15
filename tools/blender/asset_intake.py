"""Lightweight Blender asset-intake validator for Infinite Backlot.

Usage:
  blender --background --python tools/blender/asset_intake.py -- \
    --source assets/source/blender/world/kits/example.blend \
    --provenance assets/world/kits/example.provenance.json \
    --runtime-glb assets/world/kits/example.glb \
    --report output/example-intake.json
"""
import argparse
import bpy
import json
import math
import struct
from pathlib import Path

ROOT=Path(r"C:/Projects/bevy-infinite")

def glb_summary(path):
    data=path.read_bytes();magic,version,length=struct.unpack_from("<III",data,0)
    if magic!=0x46546C67 or version!=2 or length!=len(data):raise ValueError("invalid GLB header")
    offset=12;doc=None
    while offset<length:
        size,kind=struct.unpack_from("<II",data,offset);offset+=8;chunk=data[offset:offset+size];offset+=size
        if kind==0x4E4F534A:doc=json.loads(chunk.decode("utf-8").rstrip("\x00 \t\r\n"))
    if doc is None:raise ValueError("GLB JSON chunk missing")
    return {"nodes":len(doc.get("nodes",[])),"meshes":len(doc.get("meshes",[])),"materials":len(doc.get("materials",[])),"images":len(doc.get("images",[])),"cameras":len(doc.get("cameras",[])),"extensions_required":doc.get("extensionsRequired",[])}

def main():
    parser=argparse.ArgumentParser();parser.add_argument("--source",required=True);parser.add_argument("--provenance",required=True);parser.add_argument("--runtime-glb",required=True);parser.add_argument("--report",required=True);args=parser.parse_args(__import__('sys').argv[__import__('sys').argv.index('--')+1:])
    source=(ROOT/args.source).resolve() if not Path(args.source).is_absolute() else Path(args.source);provenance_path=(ROOT/args.provenance).resolve() if not Path(args.provenance).is_absolute() else Path(args.provenance);runtime=(ROOT/args.runtime_glb).resolve() if not Path(args.runtime_glb).is_absolute() else Path(args.runtime_glb);report=(ROOT/args.report).resolve() if not Path(args.report).is_absolute() else Path(args.report)
    errors=[];warnings=[]
    if not provenance_path.is_file():errors.append("provenance file missing");provenance={}
    else:provenance=json.loads(provenance_path.read_text())
    for key in ("asset_id","author","license","source_kind","modifications"):
        if not provenance.get(key):errors.append(f"provenance missing {key}")
    bpy.ops.wm.open_mainfile(filepath=str(source))
    unexpected=[]
    for o in bpy.context.scene.objects:
        if o.type in {"CAMERA","LIGHT"} and not any(c.name in {"CAMERAS","LIGHTING"} for c in o.users_collection):unexpected.append(o.name)
    if unexpected:errors.append("unexpected cameras/lights: "+", ".join(unexpected))
    absolute_links=[]
    for lib in bpy.data.libraries:
        if not lib.filepath.startswith("//"):absolute_links.append(lib.filepath)
    if absolute_links:errors.append("absolute linked libraries: "+", ".join(absolute_links))
    missing_images=[img.filepath for img in bpy.data.images if img.source=="FILE" and img.filepath and not Path(bpy.path.abspath(img.filepath)).is_file()]
    if missing_images:errors.append("missing textures: "+", ".join(missing_images))
    unsupported=[]
    for material in bpy.data.materials:
        if not material.use_nodes:unsupported.append(material.name+":no_nodes");continue
        if not any(node.type=="BSDF_PRINCIPLED" for node in material.node_tree.nodes):unsupported.append(material.name+":no_principled")
    if unsupported:errors.append("unsupported materials: "+", ".join(unsupported))
    meshes=[o for o in bpy.context.scene.objects if o.type=="MESH" and not o.hide_render]
    triangles=sum(len(o.data.loop_triangles) if o.data.loop_triangles else (o.data.calc_loop_triangles() or len(o.data.loop_triangles)) for o in meshes)
    if triangles>500000:errors.append(f"extreme polygon count: {triangles} triangles")
    invalid_scale=[]
    for o in meshes:
        dims=[abs(v) for v in o.dimensions]
        if max(dims,default=0)>100 or (max(dims,default=0)>0 and max(dims)<.01) or any(not math.isfinite(v) for v in dims):invalid_scale.append(o.name)
    if invalid_scale:errors.append("unreasonable scale: "+", ".join(invalid_scale[:20]))
    semantic_ids=[o.get("semantic_id") for o in bpy.context.scene.objects if o.get("semantic_id")]
    duplicates=sorted({x for x in semantic_ids if semantic_ids.count(x)>1})
    if duplicates:errors.append("duplicate semantic IDs: "+", ".join(duplicates))
    try:glb=glb_summary(runtime)
    except Exception as exc:errors.append(f"GLB validation failed: {exc}");glb={}
    result={"schema_version":1,"status":"PASS" if not errors else "FAIL","source":str(source.relative_to(ROOT)).replace('\\','/'),"runtime_glb":str(runtime.relative_to(ROOT)).replace('\\','/'),"provenance":str(provenance_path.relative_to(ROOT)).replace('\\','/'),"mesh_objects":len(meshes),"triangles":triangles,"materials":len(bpy.data.materials),"linked_libraries":[lib.filepath for lib in bpy.data.libraries],"unexpected_cameras_or_lights":unexpected,"missing_textures":missing_images,"duplicate_semantic_ids":duplicates,"glb":glb,"errors":errors,"warnings":warnings}
    report.parent.mkdir(parents=True,exist_ok=True);report.write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result));raise SystemExit(0 if not errors else 1)

if __name__=="__main__":main()
