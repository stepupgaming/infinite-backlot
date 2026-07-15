# Purpose

Provide Infinite Backlot's pinned Gepard text-to-speech runtime and one-load batch synthesis worker.

# Ownership

- `backlot_gepard_worker.py` is project-owned batch glue: load one session, synthesize every request, write WAVs and response JSON, emit phase JSONL, then exit for VRAM reclamation.
- `serve.py`, `cli.py`, `config.yaml`, and `Makefile` retain the upstream interactive/server surface for direct diagnosis.
- `pyproject.toml`, `.python-version`, and `uv.lock` own the reproducible Python 3.12 environment.
- `UPSTREAM.json`, `LICENSE`, and `NOTICE` own provenance and attribution.
- `gepard_inference/` and `space_reference/` have separate child contracts. `docs/`, `ref_audio/`, and `scripts/` remain directly owned here.

# Local Contracts

- Use `uv` for dependency and environment management; never use `pip` directly.
- Keep checkpoints outside the repository; the configured production root is recorded in `UPSTREAM.json` and `data/config.toml`.
- Batch requests must be a JSON array with stable IDs, output paths, text, seed, optional reference audio, and preset values.
- A successful line response records the resolved WAV, sample rate, measured duration, elapsed time, and `success: true`; the worker must fail if synthesis or writing fails.
- Standard output is JSONL progress only. Redirect upstream model prints to standard error and emit GPU-memory snapshots around load and generation.
- Seed Python, NumPy, and Torch per request so repeated inputs are reproducible within the pinned runtime.
- Preserve reference-audio licensing and privacy constraints; do not add private voice material without explicit authorization and provenance.

# Work Guidance

- Keep Backlot integration in `backlot_gepard_worker.py`; patch the vendored inference package only for a required runtime correction.
- Preserve the load-once batch shape because startup and codec/model residency dominate transaction cost.
- Keep `BACKLOT_RUNTIME.md` synchronized with worker CLI, environment isolation, model-root, and smoke instructions.

# Verification

- Run `uv sync --frozen` after dependency or lock changes.
- Run the worker against `diagnostics/gepard_smoke_requests.json` with the configured external model root and inspect both response JSON and WAV outputs.
- Run `env -u PYTHONPATH uv run --frozen --no-sync python backlot_gepard_worker.py --help` after CLI changes.

# Child DOX Index

- `gepard_inference/AGENTS.md` — vendored production inference package used by the worker and server.
- `space_reference/AGENTS.md` — retained Hugging Face Space reference snapshot.

Environment manifests, Backlot glue, docs, scripts, reference audio, and direct server tools remain owned by this file.
