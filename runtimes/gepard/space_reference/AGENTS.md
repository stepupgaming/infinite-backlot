# Purpose

Retain the pinned Hugging Face Space snapshot used to compare UI behavior, speaker assets, and alternate upstream inference integration.

# Ownership

- The Space app, interface, configuration, environment script, reference inference package, and bundled speaker embeddings are reference material.
- Production Backlot synthesis remains owned by the parent `backlot_gepard_worker.py` and sibling `gepard_inference/` package.

# Local Contracts

- Treat this subtree as a provenance-pinned reference snapshot, not the production integration point.
- Keep its source revision traceable through the parent `UPSTREAM.json`.
- Do not copy requirements or implementation changes into production without reconciling them with the parent `uv` environment and runtime contracts.
- Preserve licenses and do not add unvetted speaker embeddings or generated output.

# Work Guidance

- Prefer refreshing the snapshot coherently from its upstream source over piecemeal drift.
- Make Backlot-specific changes outside this subtree unless the task explicitly updates the reference snapshot.

# Verification


# Child DOX Index

No child DOX files yet.
