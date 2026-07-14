# Purpose

Repo-local NVIDIA Kimodo runtime used by `gemmy kimodo` for controllable SOMA 3D motion generation.

# Ownership

- Upstream Kimodo source under `kimodo/`, `MotionCorrection/`, and setup metadata remains Apache-2.0 upstream runtime code. The full upstream docs site, benchmark helpers, Docker files, and top-level demo media are intentionally not vendored because `gemmy kimodo` uses the CLI/runtime path only.
- `gemmy_run_kimodo.py` is Gemmy-owned glue: it calls upstream `kimodo.scripts.generate`, validates the expected NPZ, **requires** `<npz_stem>.studio_motion.json` export (`gemmy-kimodo-motion-v1`, 77-joint LBS track for Studio WebGL — fail the worker if export fails), renders either the default five-skin SOMA preview, a named single-skin SOMA preview, or the diagnostic skeleton preview for local inspection, and emits supervised inference/export/render phases plus the actual motion frame/waypoint counts.
- `test_gemmy_run_kimodo.py` covers real NPZ-derived motion observation metadata for both unconstrained and waypoint-constrained runs without launching inference.
- `gemmy_render_kimodo_skins.py` is Gemmy-owned preview rendering code for the five solid channel skins and named single-skin character clips. Studio Result prefers LBS over the MP4; the MP4 remains the export/fallback artifact.
- `.python-version`, `pyproject.toml`, and `uv.lock` own the reproducible native-Windows Python runtime.

# Local Contracts

- Use `uv` for environment maintenance; never use `pip`.
- Keep model checkpoints out of this repo. The default checkpoint root is `F:\Models\Kimodo`, with the recommended model under `F:\Models\Kimodo\Kimodo-SOMA-RP-v1.1`.
- Keep `TEXT_ENCODER_DEVICE=cpu` as Gemmy's default launch behavior on the RTX 5060 Ti 16 GB path unless a run explicitly asks otherwise.
- `gemmy_run_kimodo.py` must call real upstream Kimodo inference, not synthesize placeholder motion.
- Every successful Kimodo inference emits `motion_preview` using the produced NPZ frame count and the constraints JSON waypoint count (zero when unconstrained); it must not estimate motion progress or invent preview evidence.
- Preview MP4s are inspection/content-planning artifacts only; the Kimodo NPZ remains the source motion artifact. `*.studio_motion.json` is the Studio interactive Result track (expand 30→77 via SOMASkin when needed). Default previews use the five solid channel skins, named preview styles render one clean character with uniform scaling, and `--preview-style skeleton` is diagnostic.
- Studio skin asset `studio/apps/*/public/soma_skin.json` is exported from `kimodo/assets/skeletons/somaskel77/skin_standard.npz` (same mesh the NVIDIA Viser demo skins).

# Work Guidance

- Preserve upstream license, attribution files, and SPDX headers.
- Do not commit `.venv`, generated motions, rendered previews, downloaded checkpoints, or Hugging Face cache files.
- Keep Gemmy-specific patches small and isolated from upstream inference internals unless a Windows compatibility fix is required.

# Verification

- Run `uv sync` after manifest changes.
- Run a small `gemmy kimodo "A person walks." --duration 2 --output outputs\kimodo_smoke\walk.npz` smoke after runtime or command changes.
- Run `uv run python -m unittest test_gemmy_run_kimodo.py` for observation-only changes that do not require model inference.

# Child DOX Index

No child DOX files yet.
