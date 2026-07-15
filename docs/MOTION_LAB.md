# Kimodo Motion Lab

## Purpose

The motion lab answers what Kimodo motion looks like on its native SOMA body before KayKit retargeting. It is a review surface, not an automatic production-approval system.

The performer contract is `assets/motion-lab/canonical_soma_performer.json`. It uses the bundled SOMA77 standard skeleton and skin directly through `runtimes/kimodo/gemmy_render_kimodo_skins.py`; no KayKit rest correction or path warp is applied to motion-lab previews.

## Generate the broad batch

Durable prompts, seeds, categories, durations, and root waypoints live in:

```text
data/motion_lab/motion_requests.json
```

Prepare and run the batch:

```bash
uv run --no-project python tools/motion/prepare_motion_batch.py

env -u PYTHONPATH "/c/Projects/gemmy/runtimes/kimodo/.venv/Scripts/python.exe" \
  runtimes/kimodo/backlot_batch_kimodo.py \
  --checkpoint F:/Models/Kimodo/Kimodo-SOMA-RP-v1.1 \
  --requests C:/Projects/bevy-infinite/output/motion-lab/batch.request.json \
  --responses C:/Projects/bevy-infinite/output/motion-lab/batch.response.json \
  --diffusion-steps 12
```

Kimodo writes NPZ, BVH, constraints, and motion sidecars below `output/motion-lab/raw/`. `backlot_batch_kimodo.py` keeps the Kimodo/SOMA model resident for the full batch. This avoids reloading the model for every semantic while preserving an independent seed, duration, prompt, and constraint set per clip.

Import generated files into the processed production library with:

```bash
cargo run -p backlot-app --bin motion_import -- \
  --requests output/motion-lab/batch.request.json \
  --responses output/motion-lab/batch.response.json \
  --library assets/animations/library
```

The importer preserves prompt, seed, model revision, constraints, root positions, foot contacts, processing validation, raw-source paths, processed clip, and review state in `assets/animations/library/motion_lab_index.json`.

## Approval states

New structurally valid clips enter the motion-lab index as `generated` and the existing production manifest as `Pending`. A human review may set the index to:

- `usable`
- `needs_adjustment`
- `rejected`

Only a separately reviewed production action should change the production manifest to `Approved`. A clip is never approved merely because the model returned data or structural validation passed.

## Browser and showcase

Build native-SOMA previews, the index, contact sheet, and showcase:

```bash
uv run --no-project python tools/motion/build_motion_showcase.py
```

Serve the browser:

```bash
uv run --no-project python -m http.server 8765 --directory output/motion-showcase
```

Open `http://localhost:8765/`. The browser provides semantic/category filtering, loop and scrub controls, prompt/seed/duration/state, and collapsible root/contact/constraint diagnostics.

Outputs:

```text
output/motion-showcase/index.html
output/motion-showcase/motion_index.json
output/motion-showcase/motion_showcase.mp4
output/motion-showcase/contact_sheet.png
```

The showcase uses the canonical solid SOMA skin. KayKit comparison remains optional because it introduces retarget variables that should not contaminate source-motion judgment.

## Future episode selection

Episode choreography selects stable semantic IDs. The motion compiler scans `assets/animations/library`, chooses only approved clips during normal production, and may show pending clips only in explicit motion-review mode. Root-path locomotion can be warped onto a collision-safe reserved world path while preserving generated progression and contact channels.

## Known limits

- The browser currently exposes root/contact data as diagnostics rather than a live 3D overlay.
- Front/side/perspective multi-view caching is planned; the default review reel uses the established perspective SOMA renderer.
- The current production cast still lacks full general two-bone hand IK and a dedicated foot-lock solver.
- Structurally valid generated motions still require visual triage before production approval.
