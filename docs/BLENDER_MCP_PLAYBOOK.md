# Blender MCP Playbook

Practical findings recorded against Blender 5.2.0 LTS and the installed `blender_mcp` add-on v1.2 on 2026-07-15.

## Live capability audit

The installed add-on is `C:/Users/Steve/AppData/Roaming/Blender Foundation/Blender/5.2/scripts/addons/blender_mcp.py`. It exposes scene summary, object inspection with world AABBs and mesh counts, viewport screenshots, and unrestricted focused Blender Python. The Hermes MCP surface also advertises Poly Haven, Sketchfab, Hyper3D Rodin, and Hunyuan3D adapters. All four optional asset/generation integrations were disabled during this pass, so no generated or marketplace asset was silently substituted.

Blender Python provides direct mesh/object/collection/material/modifier/armature access, Geometry Nodes, UV operators, baking, library linking, glTF import/export, cameras, lights, and render configuration. The generic scene summary intentionally reports only ten objects; use focused Python queries for real audits.

Upstream checked: `ahujasid/blender-mcp` README and installed add-on source. The current upstream advertises viewport screenshots, arbitrary Python, Poly Haven, Sketchfab, Hyper3D, and Hunyuan3D. Its repository reports an MIT license. Blender MCP is a transport and execution layer, not a replacement for Blender's own data APIs.

## Underused, high-leverage workflows

1. **Measure before editing.** Query collection membership, mesh counts, evaluated world AABBs, material slots, modifier stacks, custom properties, and absolute/relative library paths in one focused Python call. Do not infer scale or overlap from names.
2. **Linked collection kitbashing.** Author reusable details as named collections in a library `.blend`, link or instance them into cells, make paths project-relative, and realize instances only for deterministic runtime export. This keeps source edits centralized while GLB consumers receive ordinary nodes.
3. **Geometry Nodes for families, not finished worlds.** Keep a seed and dimensions as named inputs, generate repeated bollards/conduits/railings/planters, preserve the modifier in editable source, and export a realized duplicate. Procedural repetition should not hide hero composition decisions.
4. **Focused material nodes.** Restrict runtime materials to glTF-safe Principled BSDF inputs and packed image textures. Blender-only procedural nodes must be baked before export. Treat emissive geometry and runtime lights as separate contracts.
5. **Semantic custom properties.** Put stable IDs, kinds, light intent, runtime-control state, and provenance on source objects. Export sidecars from these properties; do not scrape display names after the fact.
6. **Screenshot the useful camera.** Viewport screenshots are excellent for interactive modeling state but can be black or misleading when the viewport is not in a useful mode. For acceptance, create an explicit camera and render a small still from the real scene.
7. **Deterministic GLB export.** Select only the export collection, realize linked instances on an export copy, apply object transforms where needed, export with custom properties and no cameras/lights/helpers, hash the GLB, and validate node/material/texture counts. Re-export twice when exporter behavior changes.

## Best division of work

| Concern | Best owner |
|---|---|
| Hero composition, direct mesh edits, camera framing, material tuning | Blender through MCP |
| Repeated asset families, semantic extraction, deterministic exports | Versioned Blender Python |
| Runtime light spawning, world-state visibility, doors, streaming policy | Bevy |
| Source of truth for transforms and semantic locations | Blender source plus exported sidecar |
| Registry validation and world-cell compatibility | Engine-independent Rust/Python contracts |

## Direct modeling workflow

1. Inspect the target collection and world AABBs.
2. Save or work from the canonical source `.blend`.
3. Edit only the named hero collection with focused Python or direct operators.
4. Set scale, pivot, names, materials, and semantic properties immediately.
5. Render one useful human-height preview.
6. Fix one obvious failure, then export and validate.

## Procedural repetition workflow

1. Build one authored prototype.
2. Create a deterministic Geometry Nodes group with exposed count, spacing, seed, and dimensions.
3. Keep the GN object in source and create a realized export copy.
4. Confirm evaluated bounds and polygon count.
5. Register the family in the reusable catalog and provenance record.

## Sourcing and adaptation

Preferred clear-license sources are Poly Haven and ambientCG; both state that their assets are CC0 and usable commercially. Poly Haven integration is installed but disabled. If enabled later, record asset ID, source URL, download resolution, checksum, and CC0 declaration. Sketchfab is useful only when the individual asset license is explicitly compatible; the platform name alone is not provenance. Generated assets require the same intake record plus model/provider and prompt.

Every sourced/generated asset must be normalized, renamed, visually adapted, stripped of hidden cameras/lights, checked for missing textures and extreme topology, given a usable pivot and optional collision proxy, previewed, exported, and registered. Untouched stock assets do not enter hero cells.

## Materials and decals

Use Principled BSDF with base color, metallic, roughness, alpha, normal, and emission. Use UVs for image decals; use small atlas sheets for notices, repair patches, stickers, and store posters. Bake procedural noise or generated texture graphs before glTF export. Keep text and logos fictional: Odd Hours, building management, municipal departments, geometric symbols, and contradictory notices.

## Deterministic export checklist

- Project-relative linked libraries only.
- No helper cameras, lights, semantic empties, or collision displays in the export collection.
- Linked instances realized in the export copy.
- Supported Principled materials and resolvable packed textures.
- Unique semantic IDs.
- GLB import succeeds in a clean process.
- GLB SHA-256 and sidecar agree.
- Registry points to existing source, runtime, preview, and provenance files.

## Common failure modes

- Scene summary is truncated; use focused Python.
- A viewport screenshot may be black even when the scene is valid; render an explicit camera.
- Blender collection links can become absolute after save-as; run Make Paths Relative and re-save.
- Thumbnail-only backdrop meshes can overlap real master geometry; audit evaluated AABBs in composed scenes.
- Procedural nodes do not necessarily survive glTF; realize or bake them.
- Blender lights are not a reliable runtime-light contract; export semantic light intent separately.
- Camera look-at values need the same Blender-to-Bevy axis conversion as positions.
- Names can gain `.001` suffixes; semantic IDs must be custom properties and validated as unique.
- Optional Poly Haven, Sketchfab, Hyper3D, and Hunyuan tools can be advertised by MCP while disabled in Blender.
- `read_factory_settings()` can reload or orphan the MCP add-on. Prefer focused scene deletion; if the socket drops, use the pinned Blender background executable for the deterministic rebuild and record the fallback.
- This Blender 5.2 build uses render-engine identifier `BLENDER_EEVEE`, not `BLENDER_EEVEE_NEXT`.
- Blender 5.2 `GeometryNodeResampleCurve` has no legacy Python `mode` property; set its `Count` input directly.
- Blender 5.2 Actions are slotted and do not guarantee `action.fcurves`. Set `bpy.context.preferences.edit.keyframe_new_interpolation_type = "LINEAR"` before deterministic tour key insertion.

## Findings applied in this goal

The expansion pass must demonstrate at least these three findings in durable artifacts:

- Linked reusable collection instances with project-relative paths and realized GLB output.
- A deterministic Geometry Nodes repetition family retained in Blender source.
- Semantic runtime lights and world controls exported as typed metadata and instantiated in Bevy.
- Measured AABB/material/helper validation plus explicit camera renders for visual checks.
