"""Fast structural preflight for reusable world modules and motion semantics."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def load_assembler():
    path=ROOT/"tools/world/assemble_world.py"
    spec=importlib.util.spec_from_file_location("backlot_assemble_world",path)
    module=importlib.util.module_from_spec(spec); assert spec and spec.loader; spec.loader.exec_module(module)
    return module


def check_glb(path:Path)->list[str]:
    errors=[]
    data=path.read_bytes()
    if len(data)<12: return [f"{path}: truncated GLB"]
    magic,version,length=struct.unpack("<4sII",data[:12])
    if magic!=b"glTF": errors.append(f"{path}: invalid GLB magic")
    if version!=2: errors.append(f"{path}: GLB version {version}, expected 2")
    if length!=len(data): errors.append(f"{path}: declared length {length} != {len(data)}")
    return errors


def preflight(registry_path:Path,layout_path:Path,required_motions:list[str])->dict:
    errors=[]; warnings=[]
    registry=json.loads(registry_path.read_text(encoding="utf-8")); assembler=load_assembler()
    try: assembler.validate_registry(registry)
    except ValueError as error: errors.append(str(error))
    by_id={m["module_id"]:m for m in registry.get("modules",[])}
    for module in by_id.values():
        module_id=module["module_id"]
        for key in ("asset","source_blend","preview"):
            path=ROOT/module[key]
            if not path.is_file(): errors.append(f"{module_id}: missing {key} {path}")
        asset=ROOT/module["asset"]
        if asset.is_file():
            errors.extend(check_glb(asset))
            actual=hashlib.sha256(asset.read_bytes()).hexdigest()
            if actual!=module.get("glb_sha256"): errors.append(f"{module_id}: GLB hash mismatch")
        blend=ROOT/module["source_blend"]
        if blend.is_file():
            header=blend.read_bytes()[:7]
            if not (header.startswith(b"BLENDER") or header.startswith(b"\x28\xb5\x2f\xfd")):
                errors.append(f"{module_id}: invalid Blender source header")
        for group in ("sockets","staging_marks","camera_anchors","interactions"):
            ids=[point.get("id") for point in module.get(group,[])]
            if len(ids)!=len(set(ids)): errors.append(f"{module_id}: duplicate IDs in {group}")
        if module.get("category")!="skyline_proxy" and len(module.get("staging_marks",[]))<4:
            errors.append(f"{module_id}: actor-facing module needs at least four staging marks")
        if not module.get("collision_groups"): errors.append(f"{module_id}: missing collision proxy")
        provenance=module.get("provenance",{})
        if not provenance.get("author") or not provenance.get("license"): errors.append(f"{module_id}: missing provenance/license")
    layout=json.loads(layout_path.read_text(encoding="utf-8")); roles={entry["role"]:entry for entry in layout.get("instances",[])}
    for entry in roles.values():
        module=by_id.get(entry["module_id"])
        if not module: errors.append(f"{entry['role']}: missing module {entry['module_id']}")
        elif module["version"]!=entry["module_version"]: errors.append(f"{entry['role']}: module version mismatch")
    for connection in layout.get("connections",[]):
        for endpoint in ("from","to"):
            role=connection[f"{endpoint}_role"]; socket=connection[f"{endpoint}_socket"]
            if role not in roles: errors.append(f"connection references missing role {role}"); continue
            module=by_id.get(roles[role]["module_id"],{})
            if socket not in {point.get("id") for point in module.get("sockets",[])}: errors.append(f"{role}: missing socket {socket}")
    available=set()
    library=ROOT/"assets/animations/library"
    for manifest in library.glob("**/manifest.json"):
        try: available.add(json.loads(manifest.read_text(encoding="utf-8"))["semantic"])
        except (OSError,KeyError,json.JSONDecodeError) as error: warnings.append(f"could not parse {manifest}: {error}")
    for semantic in required_motions:
        if semantic and semantic not in available: errors.append(f"required motion semantic missing: {semantic}")
    return {"status":"passed" if not errors else "failed","module_count":len(by_id),"layout_instances":len(roles),"layout_connections":len(layout.get("connections",[])),"available_motion_semantics":sorted(available),"errors":errors,"warnings":warnings}


def main()->int:
    parser=argparse.ArgumentParser(); parser.add_argument("--registry",default="assets/world/registry.json"); parser.add_argument("--layout",default="data/world/demo_world_seed_424242.json"); parser.add_argument("--require-motions",default="walk,idle,panel_press"); parser.add_argument("--report",default="output/preflight/world_preflight.json"); args=parser.parse_args()
    result=preflight(ROOT/args.registry,ROOT/args.layout,[item.strip() for item in args.require_motions.split(",")])
    report=ROOT/args.report; report.parent.mkdir(parents=True,exist_ok=True); report.write_text(json.dumps(result,indent=2),encoding="utf-8")
    print(json.dumps(result,indent=2)); return 0 if result["status"]=="passed" else 1


if __name__=="__main__": raise SystemExit(main())
