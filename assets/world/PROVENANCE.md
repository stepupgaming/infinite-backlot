# Reusable World Asset Provenance

All modules registered in `assets/world/registry.json` were created for Infinite Backlot with the project-owned procedural Blender authoring script `tools/blender/build_world_kits.py` executed through Blender MCP.

- Geometry: original project-authored low-poly architecture and set dressing.
- Materials: original procedural Principled-BSDF palette; no downloaded textures.
- Source files: `assets/source/blender/world/`.
- Runtime exports: deterministic Blender glTF 2.0/GLB exports under `assets/world/`.
- License: repository license.
- External model or texture dependencies: none.
- Generator version: registry schema 1, module version 1.

Each module sidecar records its own source path, GLB SHA-256, generator, preview, and license string. Rebuilding a module changes its hash and requires running the world preflight before use.
