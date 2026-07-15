# Purpose

Own the playable character assets and the canonical mapping from story character IDs to skinned GLB scenes.

# Ownership

- `cast.ron` defines the cast schema, performer IDs, asset scenes, sizing, native clip indices, and semantic joint aliases.
- `mara.glb` and `ellis.glb` are the current performer binaries.
- `provenance.json` plus `LICENSE-KAYKIT.txt` record source, license, checksums, and import transformations.

# Local Contracts

- Character IDs, scene selectors, joint aliases, and native animation indices in `cast.ron` must match the corresponding GLB contents and runtime consumers.
- Every shipped character binary requires a provenance entry, immutable license copy, checksum, source file, source revision, and modification notes.
- Preserve the `backlot_soma77` canonical skeleton/retarget contract unless the motion and avatar layers change in the same work.
- Do not replace a GLB without updating its checksum and validating the expected skeleton, scene, materials, and native clips.

# Work Guidance

- Prefer source-preserving imports; record all transforms, mesh edits, animation changes, or tool revisions in provenance.
- Keep cast presentation metadata operational and concise because it is consumed as configuration, not marketing copy.

# Verification

- Run `uv run --no-project python tools/inspect_glb_animation.py assets/characters/<file>.glb --animation <clip>` for changed GLB animation data.
- Run the relevant Bevy scene or capture smoke after cast paths, scales, or joint aliases change.

# Child DOX Index

No child DOX files yet.
