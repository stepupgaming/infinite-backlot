"""Cheap structural preflight for the focused neighborhood environment-art pass."""
from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
REGISTRY=ROOT/"assets/world/registry.json"
CATALOG=ROOT/"assets/world/kits/infinite_backlot_detail_kit.catalog.json"
MASTER=ROOT/"assets/world/neighborhood/infinite_backlot_block.glb"
MASTER_SCENE=ROOT/"assets/world/neighborhood/infinite_backlot_block.scene.json"
HERO_IDS={"apartment_exterior_a","apartment_lobby_a","neighborhood_intersection_a","neighborhood_convenience_store_a","neighborhood_alley_a"}
REQUIRED_MASTER_MARKS={"MARK_MASTER_STREET","MARK_MASTER_ENTRANCE","MARK_MASTER_LOBBY","MARK_MASTER_ELEVATOR","MARK_MASTER_HALL","MARK_MASTER_ALLEY","MARK_MASTER_STORE"}
REQUIRED_MASTER_CAMERAS={"CAM_MASTER_STREET_WIDE","CAM_MASTER_ENTRANCE","CAM_MASTER_LOBBY","CAM_MASTER_ALLEY","CAM_MASTER_STORE"}
REQUIRED_TRANSITIONS={"TRANSITION_STREET_TO_ENTRANCE","TRANSITION_ENTRANCE_TO_LOBBY","TRANSITION_LOBBY_TO_ELEVATOR","TRANSITION_HALL_TO_ALLEY","TRANSITION_SIDEWALK_TO_STORE"}
REQUIRED_HERO_SEMANTICS={
"apartment_exterior_a":{"MARK_BUILDING_ENTRY","CAM_ENTRANCE_LOW_WIDE"},
"apartment_lobby_a":{"MARK_LOBBY_MAILBOX","MARK_FRONT_DESK","CAM_ELEVATOR_FROM_MAILBOXES"},
"neighborhood_intersection_a":{"MARK_CROSSWALK","CAM_CROSSWALK_LOW"},
"neighborhood_convenience_store_a":{"MARK_STORE_COUNTER_CUSTOMER","MARK_STORE_COUNTER_CLERK","CAM_STORE_COUNTER_TWO_SHOT"},
"neighborhood_alley_a":{"MARK_ALLEY_DUMPSTER","CAM_ALLEY_LONG_LENS"},
}


def load(path):return json.loads(path.read_text(encoding="utf-8"))

def rel(path):return str(path.relative_to(ROOT)).replace("\\","/")

def require(condition,message,errors):
    if not condition:errors.append(message)

def glb_json(path):
    raw=path.read_bytes();
    if len(raw)<20 or raw[:4]!=b"glTF":raise ValueError(f"invalid GLB header: {rel(path)}")
    total=struct.unpack_from("<I",raw,8)[0]
    if total!=len(raw):raise ValueError(f"GLB length mismatch: {rel(path)}")
    length,kind=struct.unpack_from("<II",raw,12)
    if kind!=0x4E4F534A:raise ValueError(f"GLB first chunk is not JSON: {rel(path)}")
    return json.loads(raw[20:20+length].rstrip(b" \x00").decode("utf-8"))


def semantic_ids(side):
    values=[]
    for key in ("sockets","staging_marks","camera_anchors","interactions"):
        values.extend(x["id"] for x in side.get(key,[]))
    return set(values)


def main():
    errors=[];warnings=[]
    reg=load(REGISTRY);mods=reg.get("modules",[]);ids=[m.get("module_id") for m in mods]
    require(reg.get("registry_version")==2,"registry_version must be 2",errors)
    require(len(ids)==len(set(ids)),"duplicate module IDs in registry",errors)
    require(set(reg.get("quality_tiers",[]))=={"blockout","background","production","hero"},"quality tier vocabulary incomplete",errors)
    by={m["module_id"]:m for m in mods};require(HERO_IDS<=set(by),"hero module IDs missing",errors)
    for module_id in HERO_IDS:
        module=by[module_id];require(module.get("quality_tier")=="hero",f"{module_id}: not hero tier",errors)
        for key in ("asset","source_blend","preview"):
            path=ROOT/module.get(key,"");require(path.is_file(),f"{module_id}: missing {key} {rel(path) if path.is_absolute() and path.exists() else path}",errors)
        asset=ROOT/module["asset"]
        if asset.is_file():require(hashlib.sha256(asset.read_bytes()).hexdigest()==module.get("asset_sha256"),f"{module_id}: asset hash mismatch",errors)
        side_path=asset.with_suffix(".module.json");require(side_path.is_file(),f"{module_id}: missing sidecar",errors)
        if side_path.is_file():
            side=load(side_path);present=semantic_ids(side);require(REQUIRED_HERO_SEMANTICS[module_id]<=present,f"{module_id}: missing customized semantics {sorted(REQUIRED_HERO_SEMANTICS[module_id]-present)}",errors)
            require(side.get("material_library") is not None,f"{module_id}: material library missing",errors)
    require(MASTER.is_file(),"master GLB missing",errors);require(MASTER_SCENE.is_file(),"master scene sidecar missing",errors)
    if MASTER.is_file() and MASTER_SCENE.is_file():
        side=load(MASTER_SCENE);present=semantic_ids(side)
        require(REQUIRED_MASTER_MARKS<=present,f"master marks missing: {sorted(REQUIRED_MASTER_MARKS-present)}",errors)
        require(REQUIRED_MASTER_CAMERAS<=present,f"master cameras missing: {sorted(REQUIRED_MASTER_CAMERAS-present)}",errors)
        require(REQUIRED_TRANSITIONS<=present,f"master transitions missing: {sorted(REQUIRED_TRANSITIONS-present)}",errors)
        require(side.get("asset_sha256")==hashlib.sha256(MASTER.read_bytes()).hexdigest(),"master asset hash mismatch",errors)
        doc=glb_json(MASTER);nodes={n.get("name") for n in doc.get("nodes",[])};materials=doc.get("materials",[]);meshes=doc.get("meshes",[])
        require(REQUIRED_MASTER_MARKS<=nodes,"master GLB lost staging nodes",errors);require(REQUIRED_MASTER_CAMERAS<=nodes,"master GLB lost camera nodes",errors)
        require(len(materials)>=13,f"master GLB has only {len(materials)} materials",errors);require(len(meshes)>=100,f"master GLB has only {len(meshes)} meshes",errors)
        require(not any((n or "").startswith("PREVIEW_") for n in nodes),"master GLB contains preview helper nodes",errors)
    catalog=load(CATALOG);assets=catalog.get("assets",[]);categories={a.get("category") for a in assets}
    require(len(assets)>=25,f"detail kit too small: {len(assets)}",errors);require(categories=={"architectural","street","interior"},f"detail-kit categories wrong: {categories}",errors)
    for path in [ROOT/"assets/source/blender/world/kits/infinite_backlot_material_library.blend",ROOT/"assets/source/blender/world/kits/infinite_backlot_detail_kit.blend",ROOT/"assets/source/blender/world/neighborhood/infinite_backlot_block.blend",ROOT/"assets/reference/world-art-pass/master_neighborhood.png",ROOT/"assets/reference/world-art-pass/before_after_contact_sheet.png"]:
        require(path.is_file() and path.stat().st_size>1024,f"missing or empty durable art asset: {rel(path)}",errors)
    result={"modules":len(mods),"hero_modules":len(HERO_IDS),"detail_assets":len(assets),"master_materials":len(materials) if MASTER.is_file() else 0,"master_meshes":len(meshes) if MASTER.is_file() else 0,"errors":errors,"warnings":warnings}
    print(json.dumps(result,indent=2))
    raise SystemExit(1 if errors else 0)

if __name__=="__main__":main()
