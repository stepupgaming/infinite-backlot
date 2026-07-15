# Purpose

Own offline motion ingestion, retargeting, deterministic processing, validation, transition planning, and the runtime clip library format.

# Ownership

- `bvh.rs` parses BVH and converts source motion plus sidecars.
- `soma.rs` defines the canonical SOMA-77 joint vocabulary and semantic aliases.
- `retarget.rs` maps canonical motion to performer rigs and warps root paths.
- `processing.rs` normalizes and validates clips.
- `library.rs` owns clip serialization, manifests, approval, cache keys, and lookup.
- `compiler.rs` owns motion segments, transition classification, and interaction timing.
- `src/bin/backlot-motion-compile.rs` is the asset-library compiler CLI.

# Local Contracts

- Preserve deterministic clip output and content-addressed cache keys for identical inputs.
- Validate skeleton/retarget maps, frame data, contacts, and drift before marking a clip approved.
- Keep `ProcessedMotionClip`, `MotionManifest`, `clip.motion`, and asset library schema changes synchronized.
- Treat SOMA-77 semantics and character aliases as cross-domain contracts with core avatar behavior and `assets/characters`.
- Do not make runtime playback depend on raw BVH, NPZ, or model-specific generation formats.

# Work Guidance

- Keep source import separate from processing and runtime serialization so new generators can target the canonical clip format.
- Add unit coverage for parser, retarget, processing, transition, or serialization edge cases when those paths change.

# Verification

- Run `cargo test -p backlot-motion`.
- Recompile a representative diagnostic BVH through `backlot-motion-compile` after import, retarget, validation, cache, or manifest changes.

# Child DOX Index

No child DOX files yet.
