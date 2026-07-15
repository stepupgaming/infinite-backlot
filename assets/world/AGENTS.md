# Runtime World Modules

## Scope
Registered GLBs, sidecars, hashes, and provenance consumed by deterministic world assembly.

## Rules
- `registry.json` is the catalog source of truth; module IDs and versions are stable.
- Every GLB requires an editable source, sidecar, preview, bounds, socket, staging marks, camera anchors, collision proxy, provenance, and hash.
- New ordinary modules must not require a new hard-coded Bevy scene type.
- Runtime state overrides remain outside immutable authored geometry.
- Validate with `uv run --no-project python tools/world/preflight.py` and `cargo test -p backlot-core --test world_modules`.
