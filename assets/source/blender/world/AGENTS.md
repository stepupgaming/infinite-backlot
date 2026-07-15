# Blender World Sources

## Scope
Editable reusable indoor/outdoor module sources and the assembled tour source.

## Rules
- Retain one `.blend` per registered module; do not replace sources with GLB-only assets.
- Preserve metric scale, semantic empties/cameras, cutaway groups, collision proxies, and runtime-controlled object names.
- Rebuild through `tools/blender/build_world_kits.py`; run `tools/world/preflight.py` after export.
- Do not edit the episode-0001 source set while changing this kit.
- Perform one thumbnail/contact-sheet sanity pass per batch rather than repeated subjective rerenders.
