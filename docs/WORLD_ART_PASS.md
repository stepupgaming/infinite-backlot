# Infinite Backlot Neighborhood Art Pass

Baseline: `b09db1c3e1b3eb2bbbb8d58e1a089d45c670b43d`

This pass preserves the original registry, sockets, marks, cameras, interactions, collisions, cutaways, and deterministic layout tools. It does not pretend that every original module is production art. The baseline classification is recorded in `assets/reference/world-art-pass/audit_b09db1c.json`.

## Art direction

The recurring block uses warm red masonry, aged cream plaster, deep teal paint, burgundy accents, dark metal, brass trim, cyan signage, and warm practicals. The shape language uses recessed entrances, string courses, pilasters, canopies, irregular rooflines, a service bay, mullioned storefronts, and foreground street furniture. Restrained bureaucratic jokes identify the setting: provisional elevator certification, portal-hour rules, uncertain operating hours, and a service door that should not be fed.

## Spatial contract

The master scene is one block, not a module showroom:

```text
BACKLOT AVE / crosswalk
        |
apartment sidewalk ---- ODD HOURS convenience store
        |
entrance vestibule
        |
ground-floor lobby -- mailboxes / reception / elevator
        |
first-floor hallway -- service connector -- alley
```

- Source: `assets/source/blender/world/neighborhood/infinite_backlot_block.blend`
- Runtime GLB: `assets/world/neighborhood/infinite_backlot_block.glb`
- Runtime scene data: `assets/world/neighborhood/infinite_backlot_block.scene.json`
- Editable tour: `assets/source/blender/world/neighborhood/infinite_backlot_block_tour.blend`

## Five hero upgrades

The original IDs remain stable:

- `apartment_exterior_a`: layered brick façade, recessed entry, canopy, address, AC units, fire escape, service bay, roof tank, vents, and sign.
- `apartment_lobby_a`: patterned floor, wainscot, mailboxes, reception, seating, directory, deliveries, elevator portal, practicals, and customized staging.
- `neighborhood_intersection_a`: raised sidewalk quadrants, curb language, crosswalks, drains, road repairs, lamps, street sign, hydrant, bench, planters, bike rack, and background façades.
- `neighborhood_convenience_store_a`: complete glass frontage and interior with stocked aisles, refrigerator wall, checkout, back door, window ads, and character blocking.
- `neighborhood_alley_a`: service doors, fire escape, pipes, electrical panels, dumpster, trash, deliveries, puddle, wall lights, warnings, and long-lens staging.

Before/after reference: `assets/reference/world-art-pass/before_after_contact_sheet.png`.

## Reusable libraries

- Material source: `assets/source/blender/world/kits/infinite_backlot_material_library.blend`
- Detail source: `assets/source/blender/world/kits/infinite_backlot_detail_kit.blend`
- Detail GLB: `assets/world/kits/infinite_backlot_detail_kit.glb`
- Catalog: `assets/world/kits/infinite_backlot_detail_kit.catalog.json`

The detail source has 29 linked/instanceable production collections across architectural, street, and interior categories. Materials use shared glTF-compatible Principled BSDF definitions. Visible variation comes from layered geometry, alternating masonry, trim, roughness families, grime strips, signs, advertisements, and asymmetrical dressing rather than a giant texture set.

## Rebuild

Run inside Blender 5.2 through Blender MCP:

```python
exec(compile(open(r"C:/Projects/bevy-infinite/tools/blender/build_environment_art_kit.py", encoding="utf-8").read(), "build_environment_art_kit.py", "exec"))
exec(compile(open(r"C:/Projects/bevy-infinite/tools/blender/build_neighborhood_art_pass.py", encoding="utf-8").read(), "build_neighborhood_art_pass.py", "exec"))
exec(compile(open(r"C:/Projects/bevy-infinite/tools/blender/build_neighborhood_art_tour.py", encoding="utf-8").read(), "build_neighborhood_art_tour.py", "exec"))
```

Fast structural validation:

```bash
uv run --no-project python tools/world/world_art_preflight.py
uv run --no-project python tools/world/preflight.py
```

## Runtime notes

The registry is version 2 and carries explicit `blockout`, `background`, `production`, and `hero` tiers. Hero semantics are authored from actual compositions rather than one generic camera formula. Preview-only Blender area lights are not represented by glTF; emissive practical meshes and runtime-controlled object groups survive export. Bevy should spawn its runtime lights from scene policy in a future pass.
