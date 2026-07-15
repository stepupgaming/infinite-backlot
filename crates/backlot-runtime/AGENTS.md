# Purpose

Own process lifecycle, launch specifications, and telemetry for external model runtimes.

# Ownership

- `manager.rs` owns active-runtime exclusivity, lifecycle, progress accounting, and runtime kinds.
- `process.rs` owns safe child-process spawning, exit polling, and termination.
- `llama.rs`, `gepard.rs`, `kimodo.rs`, and `parakeet.rs` map typed project requests/config into process specifications.
- `telemetry.rs` owns phase timing and the global runtime telemetry snapshot.

# Local Contracts

- The manager owns every spawned runtime from start through stop and must reclaim it on success and failure.
- Keep only the runtime needed for the current transaction active so GPU memory ownership is predictable.
- Launch arguments, environment, request/response paths, runtime kinds, and model revisions must remain explicit and typed.
- Python workers must not inherit Hermes' control-plane `PYTHONPATH`; launch specs clear it and set `PYTHONNOUSERSITE=1` so the pinned uv environment is authoritative.
- Worker stdout capture paths preserve pure JSONL traces; upstream library logs belong on stderr.
- Parse and preserve worker JSONL phase events without converting missing evidence into inferred progress.
- Keep this crate independent of Bevy and model implementation libraries; it supervises processes rather than performing inference.

# Work Guidance

- Extend typed runtime configuration before adding shell-specific command assembly elsewhere.
- Keep stop/error behavior idempotent and cover lifecycle edge cases with tests when introduced.

# Verification

- Run `cargo test -p backlot-runtime`.
- Exercise the affected owned runtime through its project smoke request after launch-spec or phase-telemetry changes.

# Child DOX Index

No child DOX files yet.
