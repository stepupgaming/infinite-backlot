# Motion-Lab Tooling

## Commands
- Prepare Kimodo batch: `uv run --no-project python tools/motion/prepare_motion_batch.py`
- Import processed clips: `cargo run -p backlot-app --bin motion_import -- --requests output/motion-lab/batch.request.json --responses output/motion-lab/batch.response.json`
- Build browser/reel: `uv run --no-project python tools/motion/build_motion_showcase.py`

## Rules
- Durable prompts, seeds, categories, durations, and constraints live in `data/motion_lab/motion_requests.json`.
- Judge source motion on the native SOMA body before KayKit retargeting.
- Structural success enters review as generated/pending, never automatically approved.
- Preserve NPZ/BVH/sidecar provenance and contact/root channels.
- Keep showcases lightweight; do not render a full episode to inspect motion.
