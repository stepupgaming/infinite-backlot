# Purpose

Provide Infinite Backlot's pinned native-Windows NeMo/Parakeet runtime for word-level speech alignment.

# Ownership

- `backlot_parakeet_worker.py` is the project-owned one-load batch worker used by the Rust production pipeline.
- `parakeet_transcribe.py` owns single-file conversion, model invocation, timestamp extraction/normalization, and JSON transcript output.
- `pyproject.toml`, `.python-version`, and `uv.lock` own the reproducible Python 3.12/CUDA environment.
- `UPSTREAM.json` records the imported source, model, execution mode, and local batch-worker addition.

# Local Contracts

- Use `uv` for every environment or dependency operation; never invoke `pip` directly.
- Keep model weights and Hugging Face caches outside the repository.
- The batch worker loads the model once, accepts a JSON request array, writes one transcript JSON per request plus aggregate response JSON, emits phase JSONL, and exits.
- Transcript payloads preserve text, words, word count, duration, provider, model ID, and device. Word timestamps must be measured/model-derived and normalized against real media duration when required.
- ffmpeg prepares mono 16 kHz audio and ffprobe supplies media duration; missing or invalid outputs must not be reported as successful alignment.
- Keep the pinned Torch/CUDA/NeMo dependency set coherent and preserve `uv run --frozen --no-sync` operation.

# Work Guidance

- Keep reusable single-file parsing and timestamp logic in `parakeet_transcribe.py`; keep batch lifecycle in the Backlot worker.
- Coordinate request, transcript, or response changes with core ASR consumers, runtime config, and diagnostics.

# Verification

- Run `uv sync --frozen` after manifest or lock changes.
- Run the worker against `diagnostics/parakeet_smoke_requests.json` and inspect aggregate responses plus each word-timing JSON.
- Run `uv run --frozen --no-sync python backlot_parakeet_worker.py --help` after CLI changes.

# Child DOX Index

No child DOX files yet.
