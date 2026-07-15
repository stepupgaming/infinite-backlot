# Purpose

Own the vendored Gepard inference package that loads checkpoints, runs the Qwen-based generator and NanoCodec, and exposes reusable synthesis sessions.

# Ownership

- Session/configuration modules own model residency and generation defaults.
- Modeling, runner, checkpoint, codec, and reference-compression modules own the inference path.
- `__init__.py` owns the public imports consumed by the Backlot worker and upstream server.

# Local Contracts

- Preserve compatibility with the pinned checkpoint and dependency set recorded by the parent runtime.
- Keep model loading separate from per-request synthesis so the parent batch worker can reuse one loaded session.
- Do not embed checkpoint weights, caches, generated audio, or machine-specific secrets in this package.
- Preserve upstream attribution and isolate project-specific orchestration in the parent worker unless an inference fix must live here.

# Work Guidance

- Compare vendored changes with `UPSTREAM.json` before editing and keep deviations small and explicit in the parent documentation or provenance.
- Keep public session/config APIs stable with their parent consumers when refactoring internals.

# Verification

- Run the parent Gepard smoke worker with the pinned external checkpoint after changes.

# Child DOX Index

No child DOX files yet.
