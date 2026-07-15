# Purpose

Provide Infinite Backlot's pinned NVIDIA Kimodo runtime and batch motion-generation contract.

# Ownership

- `backlot_run_kimodo.py` is project-owned batch glue: invoke real upstream generation, export NPZ and standard-T-pose BVH, derive the motion sidecar, write response JSON, emit phase JSONL, and exit.
- `gemmy_run_kimodo.py`, `gemmy_render_kimodo_skins.py`, and `test_gemmy_run_kimodo.py` are retained integration/reference utilities imported with this runtime.
- `.python-version`, `pyproject.toml`, `setup.py`, and `uv.lock` own the reproducible native-Windows Python environment.
- `UPSTREAM.json`, `LICENSE`, and `ATTRIBUTIONS.MD` own provenance and attribution.
- `kimodo/` and `MotionCorrection/` have separate child contracts for the vendored Python package and native extension.

# Local Contracts

- Use `uv` for environment maintenance; never use `pip`.
- Keep model checkpoints and Hugging Face caches out of this repository. The configured checkpoint is `F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1`.
- `backlot_run_kimodo.py` must call real upstream inference and fail when the requested checkpoint, NPZ, BVH, or required arrays are missing.
- Batch requests use stable prompt, duration, seed, semantic, output-stem, and optional constraint/root-waypoint fields. Root-waypoint generation requires at least two distinct frames.
- A successful response records resolved NPZ, BVH, sidecar, optional generated constraints, elapsed time, and `success: true`.
- The sidecar owns 30 Hz root positions, foot contacts, SOMA contact-joint positions, and channel names consumed by the Rust motion compiler.
- Retained preview and Studio-export utilities are diagnostic/reference paths; they must not replace the Backlot NPZ/BVH/sidecar production contract.

# Work Guidance

- Preserve upstream license, attribution files, and SPDX headers.
- Do not commit `.venv`, generated motions, rendered previews, downloaded checkpoints, or Hugging Face cache files.
- Keep project-specific changes in the root worker and keep vendored-source patches small unless an inference or Windows compatibility fix is required.

# Verification

- Run `uv sync --frozen` after dependency or lock changes.
- Run `uv run --frozen --no-sync python -m unittest test_gemmy_run_kimodo.py` for retained integration utility changes.
- Run `uv run --frozen --no-sync python backlot_run_kimodo.py --help` after batch CLI changes.
- Run the worker against `diagnostics/kimodo_smoke_requests.json` with the configured external checkpoint after inference, export, sidecar, or constraint changes.

# Child DOX Index

- `kimodo/AGENTS.md` — vendored Kimodo Python package, models, motion representation, skeletons, scripts, and visualization.
- `MotionCorrection/AGENTS.md` — vendored native motion-correction extension and Python wrapper.

Environment manifests, project integration workers, retained utilities, and provenance remain owned by this file.
