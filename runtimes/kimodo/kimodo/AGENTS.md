# Purpose

Own the vendored NVIDIA Kimodo Python package used for controllable human and humanoid motion inference, representation, post-processing, conversion, and visualization.

# Ownership

- `model/` owns model construction, checkpoint resolution, diffusion, text encoding, and registries.
- `motion_rep/`, `skeleton/`, `geometry.py`, and `postprocess.py` own canonical motion and skeleton math.
- `scripts/` owns upstream generation, conversion, locking, and demo entry points.
- `assets/` owns bundled skeleton and demo resources; `demo/`, `viz/`, `exports/`, and `metrics/` own upstream authoring, inspection, export, and evaluation surfaces.

# Local Contracts

- Preserve compatibility with the pinned upstream revision, checkpoint family, SOMA-77 skeleton, and parent Python lock.
- Keep checkpoint weights, model caches, generated motions, previews, and server output outside the repository.
- Do not bypass real model inference with synthetic motion in production entry points.
- Preserve array shapes, skeleton semantics, coordinate conventions, and CLI output behavior consumed by the parent worker.
- Keep upstream license/SPDX content intact and isolate project orchestration in the parent runtime when possible.

# Work Guidance

- Compare changes against `../UPSTREAM.json` and document any durable project deviation in the parent contract or provenance.
- When changing skeleton, representation, generation, or conversion behavior, trace every parent worker field and Rust motion consumer before editing.

# Verification

- Run the parent Kimodo smoke worker against the pinned checkpoint after model, generation, skeleton, motion-representation, post-processing, or export changes.

# Child DOX Index

No child DOX files yet.
