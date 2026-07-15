# Apartment Floor 03 — provenance and license

## Ownership

- The set geometry, materials, semantic markers, camera anchors, lighting references, and reference renders were authored specifically for Infinite Backlot on 2026-07-14.
- No downloaded meshes, textures, HDRIs, fonts, or third-party material libraries are embedded in the `.blend` or exported GLB.
- Project-authored set content follows the repository's own license. Blender 5.2 and the `ahujasid/blender-mcp` add-on were authoring tools; their software licenses do not transfer third-party content into this asset.

## Reproducible source chain

1. `tools/blender/build_apartment_floor_03.py` creates the scene, semantic nodes, cameras, materials, references, and editable source.
2. `assets/source/blender/apartment_floor_03.blend` is the editable source of truth.
3. `tools/blender/export_apartment_floor_03.py` validates the source and writes:
   - `assets/scenes/apartment_floor_03.glb`
   - `assets/scenes/apartment_floor_03.scene.json`
4. `assets/reference/apartment_floor_03/*.png` are Blender reference renders, not runtime inputs.

## Export record

- Blender: 5.2.0, Windows x64.
- Coordinate conversion: Blender `(X, Y, Z)` to Bevy `(X, Z, -Y)`, meters.
- Exported GLB SHA-256: `c31e46a7270f7d88a40f34e4aa74791f90b4c30d3bb93058e60f907608d66941`.
- Two consecutive final exports produced the same SHA-256.
- The sidecar is the semantic contract for required nodes, authored cameras, staging marks, dynamic doors/indicator, panel buttons, props, cutaways, collision/navigation volumes, and lighting references.
