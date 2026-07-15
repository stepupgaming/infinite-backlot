# Purpose

Own the media and authored content loaded by Infinite Backlot at runtime.

# Ownership

- `animations/` contains compiled, content-addressed motion clips and their generation metadata.
- `characters/` contains cast registration, GLB performers, licenses, and provenance.
- `audio/sfx/` contains authored sound effects used by the episode renderer.
- `source/blender/` contains editable Blender set sources and provenance records.
- `scenes/` contains deterministic runtime GLBs and their semantic sidecars.
- `reference/` contains authored visual-review renders; these are evidence, not runtime inputs.
- `locations/`, `materials/`, `props/`, and `shows/` are reserved asset-domain scaffolds and remain owned here until they acquire distinct contracts.

# Local Contracts

- Treat paths beneath `assets/` as runtime-facing identifiers; coordinate renames with every Rust or data reference.
- Ship only assets with compatible usage rights and retain license, source, revision, checksum, and transformation records where applicable.
- Keep model weights, caches, intermediate renders, and raw motion-generation output outside this tree. `animations/library/**/raw` is ignored.
- Do not hand-edit opaque binary assets when a reproducible source or compiler path exists.
- A Blender-authored set must keep its `.blend`, deterministic build/export scripts, semantic sidecar, GLB hash, and provenance synchronized.
- Runtime set sidecars are fail-closed contracts: missing or duplicate required semantic nodes must block imported-set startup rather than silently changing behavior.

# Work Guidance

- Keep descriptive registries and manifests beside the artifacts they govern.
- Add a child DOX file when one of the currently reserved asset domains gains its own schema, workflow, or quality gate.

# Verification

- Run the nearest child verification for character or motion changes.
- Run `cargo test -p backlot-core` when asset path or loading contracts change.
- Run `cargo test -p backlot-app backlot_scene::tests` for scene sidecar or semantic-node changes.
- Re-export a Blender set twice and compare GLB SHA-256 values after source or exporter changes.

# Child DOX Index

- `animations/AGENTS.md` — content-addressed motion clips, manifests, and validation evidence.
- `characters/AGENTS.md` — performer GLBs, cast mapping, and source provenance.

Audio, location, material, prop, and show assets remain directly owned by this file.
