# Purpose

Own the local external-model runtimes supervised by Infinite Backlot.

# Ownership

- `gepard/` provides project-owned batch TTS around a vendored Gepard inference stack.
- `kimodo/` provides project-owned batch motion generation around vendored NVIDIA Kimodo.
- `llama.cpp/` is a pinned Windows binary distribution used for the owned Gemma authoring server.
- `parakeet-asr/` provides the pinned NeMo/Parakeet word-alignment worker.

# Local Contracts

- Keep model weights and Hugging Face caches outside the repository; record their expected locations and upstream revisions in manifests or config.
- Every long-lived or GPU-heavy runtime must be launched and reclaimed through the Rust runtime manager or an explicit diagnostic command.
- Batch workers consume durable JSON request files, emit machine-readable JSONL phase events on stdout, write durable response JSON, and fail nonzero on incomplete work.
- Preserve upstream licenses, notices, provenance manifests, and clear separation between project-owned glue and vendored code.
- Use `uv` for every Python environment and dependency operation. Do not invoke `pip` directly.
- Never commit `.venv`, `__pycache__`, downloaded checkpoints, generated motions, generated audio, or runtime output directories.
- Commit project-owned runtime workers, typed contracts, environment manifests/locks, instructions, provenance, and small text fixtures. Exclude large or binary runtime artifacts unless a separately reviewed provenance policy explicitly permits them.

# Work Guidance

- Make integration changes in the runtime root worker first; patch vendored internals only when the runtime itself requires it.
- Keep request/response changes synchronized with `crates/backlot-runtime`, `crates/backlot-core`, configuration, and diagnostic fixtures.

# Verification

- Run the nearest runtime child verification after worker, manifest, dependency, or vendored-runtime changes.
- Run `cargo test -p backlot-runtime` when launch arguments, lifecycle behavior, or telemetry contracts change.

# Child DOX Index

- `gepard/AGENTS.md` — Gepard TTS environment, batch worker, and vendored inference sources.
- `kimodo/AGENTS.md` — Kimodo motion environment, batch worker, exports, and vendored sources.
- `llama.cpp/AGENTS.md` — pinned native llama.cpp server binary bundle.
- `parakeet-asr/AGENTS.md` — pinned Parakeet/NeMo ASR environment and batch alignment worker.
