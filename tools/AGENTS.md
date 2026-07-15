# Purpose

Own small repository-local developer utilities that inspect or transform project material without becoming runtime dependencies.

# Ownership

- `inspect_glb_animation.py` reads GLB v2 animation channels and can calculate relaxed upper-arm rotations for character inspection.
- `blender/build_apartment_floor_03.py` deterministically authors the apartment-floor set, semantic nodes, cameras, and reference renders.
- `blender/export_apartment_floor_03.py` validates the Blender source and exports the runtime GLB plus semantic sidecar.
- Future general-purpose repository utilities remain owned here until a subdomain develops its own workflow and child contract.

# Local Contracts

- Keep tools deterministic, explicit about inputs and outputs, and safe to run from the repository root.
- Do not silently mutate source assets; inspection tools should be read-only unless their CLI clearly declares an output.
- Avoid adding a dependency when the standard library is sufficient. Manage any required Python dependency through `uv`, never `pip`.
- Runtime production behavior belongs in the owning crate or runtime, not in an ad hoc tool.
- Blender set exporters must include every required semantic node in exactly one sidecar category and preserve byte-identical GLB output across consecutive exports from the same `.blend`.

# Work Guidance

- Include actionable CLI help and return nonzero on malformed inputs.
- Update the owning asset or data contract when a tool establishes a new durable artifact format.

# Verification

- Run `uv run --no-project python tools/inspect_glb_animation.py --help` after CLI changes.
- Run the tool against a representative GLB after parsing or transform changes.
- Run both Blender set scripts with Blender 5.2, then compare two consecutive GLB SHA-256 values and run `cargo test -p backlot-app backlot_scene::tests`.

# Child DOX Index

No child DOX files yet.
