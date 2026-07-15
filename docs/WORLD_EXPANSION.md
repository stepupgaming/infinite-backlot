# Infinite Backlot World Expansion

Baseline: `6948fcd8adc03d9603021b79a97a6dd435e38243`.

## Durable package

The expansion keeps the original connected block as the source of truth and adds three reusable hero cells:

- `cell_street_extension`: Hinge & Hour corner business, loading frontage, procedural bollards, traffic furniture, and east/west continuation.
- `cell_public_transit_pocket`: municipal shelter, waiting/conversation marks, notice board, newsstand, planters, and transit socket.
- `cell_industrial_transition`: rail underpass, drainage/service road, bridge structure, utility shed, overbuilt pipes, and outskirts socket.

Editable sources live under `assets/source/blender/world/cells/`; runtime GLBs and semantic sidecars live under `assets/world/cells/`.

`infinite_backlot_expanded_world.blend` uses project-relative linked collections for the original block and all three cells. The runtime GLB is a realized export so Bevy and other glTF consumers receive normal nodes and meshes. The linked editable source remains portable and is not replaced by the realized export scratch state.

## Blender MCP findings applied

1. Focused Blender Python drove named direct modeling, semantic authoring, previews, and deterministic GLB export while MCP screenshots/scene inspection supplied the visual feedback loop.
2. `GN_STREET_BOLLARD_ROW` retains a Geometry Nodes repetition family in the editable street-cell source; export applies the evaluated result.
3. The 21 reusable expansion collections are marked as Blender Asset Browser assets and cataloged.
4. The expanded master uses project-relative linked collections rather than copied geometry.
5. Typed light intents remain semantic data; Bevy creates the runtime lights.

## Runtime proof

Run:

```bash
cargo run -p backlot-app --bin backlot -- --connected-world-proof
```

The command resolves `infinite_backlot_block` from `assets/world/registry.json`, derives the exact `.scene.json`, loads the Blender GLB through `WorldAssetRoot`, registers semantics, binds the two openable doors, creates typed practical lights, loads Mara, and captures the deterministic route. No Rust primitive duplicate of the neighborhood is created.

A cheaper static policy check is available with:

```bash
cargo run -p backlot-app --bin backlot -- --connected-world-lighting-preview
```

## Cells and future streaming

`assets/world/cells/world_cells.json` defines six meaningful cell contracts and the socket vocabulary. It does not claim a complete streaming system. Bounds, compatibility, state hooks, priorities, backgrounds, cameras, staging, interactions, and lighting policies are authored now so later streaming work does not need to reinterpret raw meshes.

## Honest limitations

- The three new cells are deliberate medium-distance production vocabulary, not close-up film sets.
- Materials are geometry/color driven; no bespoke texture-baking pass was required in this iteration.
- Collision proxies remain coarse staging guards rather than a navmesh or physics-authoring system.
- The Bevy proof keeps runtime-controlled entry doors open by hiding their exported root nodes; articulated door animation is future work.
- The connected proof uses a native KayKit walk clip sampled over the authored path. It does not claim a new Kimodo retarget or foot-lock solution.
- The expanded master is a linked Blender composition and a realized GLB, not a complete runtime streaming implementation.
- The connected portion of `world_expansion_tour.mp4` is the Blender tour through transit, the original block, and the street extension. The composed-master camera still entered linked skyline geometry on the industrial tail after the single allowed corrective render, so the final 14 seconds use a slow move over the already accepted industrial-cell hero preview. This is an explicit preview compromise, not evidence that the linked-master industrial camera route is solved.
