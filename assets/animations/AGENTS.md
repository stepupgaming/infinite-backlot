# Purpose

Own the compiled motion library consumed by the runtime and renderer.

# Ownership

- `library/<semantic>/<cache-key>/clip.motion` is the deterministic processed clip.
- Each adjacent `manifest.json` records generation identity and approval; `validation.json`, when present, records compiler evidence.
- Semantic folders such as `idle`, `walk`, `talk`, and interaction motions are lookup categories, not independent ownership boundaries.

# Local Contracts

- Preserve the content-addressed layout produced by `backlot-motion-compile`; the directory key must match the compiler's inputs and manifest `cache_key`.
- A shippable clip requires `schema_version`, semantic, source revision, checkpoint identity, prompt, seed, approval state, and clip path.
- Only mark a clip `approved` after deterministic processing and validation succeed. Retain source BVH/sidecar references in validation evidence when available.
- `clip.motion` is the runtime source; NPZ, BVH, previews, and raw generation output belong in diagnostics or ignored output, not beside approved clips.
- Coordinate schema or skeleton changes with `backlot-motion`, character retarget mappings, and all existing library entries.

# Work Guidance

- Generate or recompile clips through the motion compiler instead of editing binary payloads or cache keys by hand.
- Keep semantic names stable because authored actions and runtime lookup use them as identifiers.

# Verification

- Run `cargo test -p backlot-motion` after format, processing, retarget, or library changes.
- Re-run `cargo run -p backlot-motion --bin backlot-motion-compile -- <required flags>` for changed source motion and require a valid `validation.json` before approval.

# Child DOX Index

No child DOX files yet.
