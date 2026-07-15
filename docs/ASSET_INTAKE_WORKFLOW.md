# Asset Intake Workflow

Use this for every sourced, generated, or project-authored reusable asset family.

## Flow

1. **Acquire or generate.** Accept only an explicit license. Prefer project-owned work or CC0 sources listed in `BLENDER_MCP_PLAYBOOK.md`.
2. **Record provenance first.** Create a JSON file containing `asset_id`, `author`, `license`, `source_kind`, `commercial_use`, source URLs, and a concrete `modifications` list.
3. **Import into a scratch Blender file.** Remove unexpected cameras, lights, rigs, empties, and hidden duplicates.
4. **Normalize.** Work in meters; apply scale/rotation where appropriate; make +Z up in Blender and let glTF convert to Bevy Y-up. Put a usable pivot at the floor/contact point.
5. **Clean and adapt.** Remove broken or extreme topology, correct normals, simplify excess geometry, rename predictably, and change silhouette/details so stock sources fit the Infinite Backlot bible.
6. **Adapt materials.** Use Principled BSDF materials that export to glTF. Pack or copy permissively licensed textures under the asset directory; never rely on an absolute external texture path.
7. **Add semantics.** Assign unique IDs, optional collision proxies, interaction points, sockets, and runtime-control metadata.
8. **Library and preview.** Mark the collection as a Blender Asset Browser asset, save an editable `.blend`, and produce one useful preview.
9. **Export and register.** Export a deterministic GLB with extras and applied evaluated modifiers; update the catalog/registry and SHA-256.
10. **Run intake validation.** Fix errors once; document non-blocking warnings.

## Command

```bash
"C:/Program Files/Blender Foundation/Blender 5.2/blender.exe" --background \
  --python tools/blender/asset_intake.py -- \
  --source assets/source/blender/world/kits/infinite_backlot_expansion_assets.blend \
  --provenance assets/world/kits/infinite_backlot_expansion_assets.provenance.json \
  --runtime-glb assets/world/kits/infinite_backlot_expansion_assets.glb \
  --report assets/reference/world-expansion/asset_intake_report.json
```

The validator checks required provenance fields, project-relative linked libraries, supported Principled materials, missing textures, unreasonable scale, extreme polygon count, hidden unexpected cameras/lights, duplicate semantic IDs, and GLB structural readability. It is deliberately a guardrail, not an asset-management application.
