"""Controlled CC0 intake from the official Poly Haven API.

Downloads project-selected 1K source files, verifies API MD5 checksums, and emits
an immutable provenance manifest. This is the fallback when Blender's Poly Haven
MCP integration is not connected.
"""
from __future__ import annotations

import hashlib
import json
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "assets/source/polyhaven"
MANIFEST = ROOT / "assets/world/kits/polyhaven_cc0_intake.provenance.json"
API = "https://api.polyhaven.com"
SELECTIONS = {
    "models": ["CashRegister_01", "plastic_crate_01", "metal_tool_chest", "painted_wooden_bench", "street_lamp_01"],
    "textures": ["asphalt_02", "chipped_concrete", "metal_grate_rusty", "brick_floor_003", "green_metal_rust"],
    "hdris": ["urban_alley_01", "industrial_sunset", "abandoned_bakery"],
}


def request(url):
    return urllib.request.Request(url, headers={"User-Agent":"InfiniteBacklot-AssetIntake/1.0","Accept":"application/json,*/*"})


def api_json(url):
    with urllib.request.urlopen(request(url), timeout=90) as response:
        return json.load(response)


def download(url, path, md5):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or hashlib.md5(path.read_bytes()).hexdigest() != md5:
        with urllib.request.urlopen(request(url), timeout=180) as response:
            path.write_bytes(response.read())
    actual = hashlib.md5(path.read_bytes()).hexdigest()
    if actual != md5:
        raise ValueError(f"checksum mismatch for {path}: {actual} != {md5}")
    return {"path": path.relative_to(ROOT).as_posix(), "bytes": path.stat().st_size, "md5": actual, "url": url}


def leaf(entry):
    if isinstance(entry, dict) and "url" in entry and "md5" in entry:
        return entry
    if isinstance(entry, dict):
        for value in entry.values():
            found = leaf(value)
            if found:
                return found
    return None


def model_files(asset_id, files):
    package = files["gltf"]["1k"]["gltf"]
    selected = [(f"{asset_id}_1k.gltf", package)]
    selected.extend((name, value) for name, value in package.get("include", {}).items())
    return selected


def texture_files(asset_id, files):
    selected = []
    for channel in ["Diffuse", "nor_gl", "arm", "Rough", "Displacement"]:
        if channel not in files or "1k" not in files[channel]:
            continue
        item = leaf(files[channel]["1k"])
        if item:
            suffix = Path(item["url"]).suffix
            selected.append((f"{asset_id}_{channel.lower()}_1k{suffix}", item))
    return selected


def hdri_files(asset_id, files):
    candidates = []
    for channel in ["hdri", "HDRI", "exr", "hdr"]:
        if channel in files and "1k" in files[channel]:
            item = leaf(files[channel]["1k"])
            if item:
                candidates.append((f"{asset_id}_1k{Path(item['url']).suffix}", item))
    if not candidates:
        # Poly Haven HDRIs normally expose `hdri/1k/hdr`; recurse only inside 1K records.
        for key, value in files.items():
            if isinstance(value, dict) and "1k" in value:
                item = leaf(value["1k"])
                if item and Path(item["url"]).suffix.lower() in {".hdr", ".exr"}:
                    candidates.append((f"{asset_id}_1k{Path(item['url']).suffix}", item))
                    break
    return candidates[:1]


def main():
    asset_index = api_json(f"{API}/assets")
    records = []
    for asset_type, asset_ids in SELECTIONS.items():
        for asset_id in asset_ids:
            metadata = asset_index[asset_id]
            files = api_json(f"{API}/files/{asset_id}")
            selected = model_files(asset_id, files) if asset_type == "models" else texture_files(asset_id, files) if asset_type == "textures" else hdri_files(asset_id, files)
            if not selected:
                raise ValueError(f"no supported 1K files for {asset_id}")
            downloaded = [download(item["url"], DEST / asset_id / name, item["md5"]) for name, item in selected]
            records.append({
                "asset_id": asset_id,
                "name": metadata.get("name", asset_id),
                "asset_type": asset_type,
                "source": f"https://polyhaven.com/a/{asset_id}",
                "api": f"{API}/files/{asset_id}",
                "license": "CC0 1.0 Universal",
                "license_url": "https://polyhaven.com/license",
                "resolution": "1k",
                "categories": metadata.get("categories", []),
                "files": downloaded,
                "planned_adaptations": ["palette unification", "material simplification", "runtime naming", "simple collider", "Infinite Backlot kitbash variants"],
            })
    MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(json.dumps({"schema_version":1,"source":"Poly Haven official API","license":"CC0 1.0 Universal","assets":records}, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({"assets":len(records),"files":sum(len(r["files"]) for r in records),"manifest":str(MANIFEST)}))


if __name__ == "__main__":
    main()
