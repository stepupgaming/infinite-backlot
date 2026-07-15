# World Module Workflow

## Implemented asset boundary

Reusable environment geometry is authored and retained in Blender. Bevy/runtime code selects registered modules, applies transforms and state, places actors, and executes cameras; it does not recreate these modules from Rust cubes.

Current roots:

```text
assets/source/blender/world/apartment_building/*.blend
assets/source/blender/world/neighborhood/*.blend
assets/world/apartment_building/*.glb
assets/world/neighborhood/*.glb
assets/world/registry.json
assets/reference/world-modules/*.png
```

Each module has one editable `.blend`, one GLB, one `.module.json` sidecar, and one low-cost thumbnail. Sources are intentionally independent files so one module can be revised without opening a monolithic city scene.

## Blender MCP authoring

Blender MCP is the preferred interactive authoring path. The current kit was built with:

```python
exec(compile(open(
    r"C:/Projects/bevy-infinite/tools/blender/build_world_kits.py",
    encoding="utf-8",
).read(), "build_world_kits.py", "exec"))
```

The same script supports deterministic noninteractive rebuilds:

```bash
"/c/Program Files/Blender Foundation/Blender 5.2/blender.exe" \
  --background --python tools/blender/build_world_kits.py
```

A module rebuild overwrites only the generated world-kit sources, GLBs, sidecars, and thumbnails. It does not touch the episode-0001 source set.

## Semantic authoring contract

Modules expose typed empties and cameras:

- Connection sockets: `SOCKET_*`, `ROAD_*`, `SIDEWALK_*`, `ALLEY_*`, `LOT_*`, `BUILDING_ENTRANCE_*`, `TRANSIT_*`.
- Actor marks: `MARK_ENTRY`, `MARK_EXIT`, `MARK_CONVERSATION_A/B`, `MARK_OBSERVER`, `MARK_REVEAL_CLEAR`, plus module-specific marks.
- Camera anchors: `CAM_WIDE`, `CAM_TWO_SHOT`, `CAM_INTERACTION`, `CAM_REVEAL`.
- Runtime interactions: `INTERACT_*`.
- Hideable geometry: `CUTAWAY_*`.
- Collision proxies: `COLLIDER_*`.

Static architecture and runtime-controlled doors, panels, buttons, lights, and props are named separately in the GLB. Custom properties are exported as glTF extras.

## Registry

`assets/world/registry.json` is the durable catalog. It contains paths, versions, bounds, sockets, marks, camera anchors, interactions, cutaways, collision groups, tags, provenance, previews, and GLB hashes.

Rust can load and validate it through `backlot_core::world_modules::WorldModuleRegistry`; new module entries do not require a new Rust scene type.

## Deterministic assembly

Generate the demonstration layout with:

```bash
uv run --no-project python tools/world/assemble_world.py \
  --seed 424242 \
  --output data/world/demo_world_seed_424242.json
```

The committed seed creates 19 instances and 17 explicit socket connections spanning street, entrance, lobby, elevator, two interior floor arrangements, stairs/service, alley, storefront, a hero store shell, park, courtyard, and skyline proxy. The same seed reproduces the same fingerprint. Different seeds vary reviewed floor and service modules, not raw mesh geometry.

This first assembler writes an explicit layout. Chunk streaming, automatic geometric socket snapping, navigation baking, and persistence migrations are planned later.

## Validation and previews

Fast preflight:

```bash
uv run --no-project python tools/world/preflight.py
cargo test -p backlot-core --test world_modules
```

The preflight checks GLB headers and hashes, Blender source presence, bounds, unique semantics, collision proxies, staging/camera coverage, provenance, layout module versions, connection sockets, and required motion semantics.

The module contact sheet is:

```text
assets/reference/world-modules/contact_sheet.png
```

The one low-cost tour is built from registered GLBs, not duplicate Rust geometry:

```bash
"/c/Program Files/Blender Foundation/Blender 5.2/blender.exe" \
  --background assets/source/blender/world/demo_world_tour.blend --render-anim
ffmpeg -framerate 12 -i output/world-tour/frames/frame_%04d.png \
  -c:v libx264 -pix_fmt yuv420p output/world-tour/world_tour.mp4
```

## Adding a module

1. Add or revise a `ModuleSpec` and builder in `tools/blender/build_world_kits.py`, or author an equivalent contract-compliant `.blend` through Blender MCP.
2. Export a GLB with extras, cameras, transforms applied, Y-up conversion, and no external absolute dependencies.
3. Write/generate the sidecar and registry entry.
4. Generate one thumbnail.
5. Run `tools/world/preflight.py` and the Rust registry test.
6. Perform one visual sanity check, fix obvious breakage, and move on.

Do not add a custom Bevy scene implementation for ordinary locations.
